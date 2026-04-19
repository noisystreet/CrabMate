//! 工具名 → [`ToolExecutionClass`] / [`ToolDispatchMeta`] / 内部分发 [`HandlerId`]（`tool_dispatch_registry!`）。

use std::collections::HashMap;
use std::sync::OnceLock;

/// 工具在运行时的执行类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionClass {
    Workflow,
    CommandSpawnTimeout,
    WeatherSpawnTimeout,
    WebSearchSpawnTimeout,
    HttpFetchSpawnTimeout,
    BlockingSync,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ToolDispatchMeta {
    pub name: &'static str,
    pub requires_workspace: bool,
    pub class: ToolExecutionClass,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HandlerId {
    Workflow,
    RunCommand,
    GetWeather,
    WebSearch,
    HttpFetch,
    HttpRequest,
    SyncDefault,
}

/// 由 `tool_dispatch_registry!` 展开：生成 `DISPATCH_METADATA` 与 `handler_dispatch_map_build`，与 `HANDLER_MAP` 同源。
macro_rules! tool_dispatch_registry {
    ( $( ( $name:literal, $reqws:expr, $class:ident, $handler:ident ) ),* $(,)? ) => {
        static DISPATCH_METADATA: &[ToolDispatchMeta] = &[
            $(
                ToolDispatchMeta {
                    name: $name,
                    requires_workspace: $reqws,
                    class: ToolExecutionClass::$class,
                },
            )*
        ];

        fn handler_dispatch_map_build() -> HashMap<&'static str, HandlerId> {
            let mut m = HashMap::new();
            $(
                m.insert($name, HandlerId::$handler);
            )*
            m
        }
    };
}

tool_dispatch_registry! {
    ("workflow_execute", false, Workflow, Workflow),
    ("run_command", true, CommandSpawnTimeout, RunCommand),
    ("get_weather", false, WeatherSpawnTimeout, GetWeather),
    ("web_search", false, WebSearchSpawnTimeout, WebSearch),
    ("http_fetch", false, HttpFetchSpawnTimeout, HttpFetch),
    ("http_request", false, HttpFetchSpawnTimeout, HttpRequest),
}

/// 注册表中显式声明的工具；其余名称运行时走 `SyncDefault`（同步 `run_tool`）。
/// 与 `handler_id_for` / `HANDLER_MAP` 共用 `tool_dispatch_registry!` 生成的表，勿分开维护。
pub fn all_dispatch_metadata() -> &'static [ToolDispatchMeta] {
    DISPATCH_METADATA
}

static HANDLER_MAP: OnceLock<HashMap<&'static str, HandlerId>> = OnceLock::new();

pub(crate) fn handler_id_for(name: &str) -> HandlerId {
    HANDLER_MAP
        .get_or_init(handler_dispatch_map_build)
        .get(name)
        .copied()
        .unwrap_or(HandlerId::SyncDefault)
}

fn meta_by_name(name: &str) -> Option<&'static ToolDispatchMeta> {
    all_dispatch_metadata().iter().find(|m| m.name == name)
}

/// 若在 `all_dispatch_metadata` 中登记则返回其元数据，否则 `None`（运行时走同步 `run_tool`）。
pub fn try_dispatch_meta(name: &str) -> Option<&'static ToolDispatchMeta> {
    meta_by_name(name)
}

/// 合并「注册表元数据 + 默认同步」的执行类别，便于文档或将来生成 OpenAPI。
pub fn execution_class_for_tool(name: &str) -> ToolExecutionClass {
    try_dispatch_meta(name)
        .map(|m| m.class)
        .unwrap_or(ToolExecutionClass::BlockingSync)
}
