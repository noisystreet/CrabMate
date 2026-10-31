//! 工具名 → [`ToolExecutionClass`] / [`ToolDispatchMeta`] / 内部分发 [`HandlerId`]（`tool_dispatch_registry!`）。

use std::collections::HashMap;
use std::sync::Arc;

use crate::cm_names as tool_names;

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
pub enum HandlerId {
    Workflow,
    RunCommand,
    TerminalSession,
    GetWeather,
    WebSearch,
    HttpFetch,
    HttpRequest,
    SyncDefault,
}

// 名字接受表达式而非字面量：表体统一引用 `tool_names` 常量（#1155）。
/// 由 `tool_dispatch_registry!` 展开：生成 `DISPATCH_METADATA` 与 `handler_dispatch_map_build`。
macro_rules! tool_dispatch_registry {
    ( $( ( $name:expr, $reqws:expr, $class:ident, $handler:ident ) ),* $(,)? ) => {
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
    (tool_names::WORKFLOW_EXECUTE, false, Workflow, Workflow),
    (tool_names::RUN_COMMAND, true, CommandSpawnTimeout, RunCommand),
    (tool_names::TERMINAL_SESSION, true, CommandSpawnTimeout, TerminalSession),
    (tool_names::GET_WEATHER, false, WeatherSpawnTimeout, GetWeather),
    (tool_names::WEB_SEARCH, false, WebSearchSpawnTimeout, WebSearch),
    (tool_names::HTTP_FETCH, false, HttpFetchSpawnTimeout, HttpFetch),
    (tool_names::HTTP_REQUEST, false, HttpFetchSpawnTimeout, HttpRequest),
}

/// 注册表中显式声明的工具；其余名称运行时走 `SyncDefault`（同步 `run_tool`）。
pub fn all_dispatch_metadata() -> &'static [ToolDispatchMeta] {
    DISPATCH_METADATA
}

/// 工具名 → [`HandlerId`] 查找表；由进程级 `ProcessHandles` 等持有，避免模块级 `static`。
#[derive(Debug, Clone)]
pub struct HandlerLookupTable {
    map: Arc<HashMap<&'static str, HandlerId>>,
}

impl HandlerLookupTable {
    /// 生产默认表（与历史 `HANDLER_MAP` 单次初始化结果一致）。
    pub fn default_dispatch() -> Self {
        Self {
            map: Arc::new(handler_dispatch_map_build()),
        }
    }

    pub fn id_for(&self, name: &str) -> HandlerId {
        self.map
            .get(name)
            .copied()
            .unwrap_or(HandlerId::SyncDefault)
    }
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
