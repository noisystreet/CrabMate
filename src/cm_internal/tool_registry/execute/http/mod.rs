//! 出站网络工具的执行与网络闸门。
//!
//! 原为 `http_tool_exec.inc.rs`（闸门）与 `execute_http_tools.inc.rs`（工具入口）两片，
//! 现为 `gate` / `tools` 两个真子模块（#1198）。两者合计仍远低于 `file ≤ 920`，之所以
//! 不合并：`gate` 持有网络轴的测试模块，且被 `docs/design/permission_levels.md` 多处单独
//! 锚定——合成一体会把这条边界重新糊掉。

mod gate;
mod tools;

// 显式接口（#1198）：兄弟子模块与 `run_command` 只能经此取用本目录的项。
// `gate` 的闸门接口供兄弟 `tools` 与 `run_command` 的并行预取共用。
pub(in crate::cm_internal::tool_registry::execute) use gate::{
    HttpToolPreflightParams, http_fetch_prefetch_approval, http_tool_preflight,
    spawn_blocking_http_tool,
};
pub(in crate::cm_internal::tool_registry::execute) use tools::{
    execute_get_weather_web, execute_http_fetch_impl, execute_http_request_impl,
    execute_web_search_web,
};
