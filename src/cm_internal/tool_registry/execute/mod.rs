//! `dispatch_tool` 及各类需异步/阻塞池的工具执行实现。
//!
//! 进入阻塞池前对 [`AgentConfig`] 使用 [`Arc::clone`]；白名单等同理。详见仓库 `tool_registry` 模块说明。
//!
//! ## 子模块（#1198）
//!
//! 各执行阶段原先都是 `include!` 分片，文本被拼进本模块的**同一作用域**：跨阶段调用靠裸名，
//! 改一片漏改另一片编译器也无从发现（#1158 / #1198）。现在每个阶段是独立子模块，兄弟之间
//! 只能经下方**显式接口清单**互相调用——`use …::*;` 的 glob 重导出已移除。
//!
//! - `dispatch`：`dispatch_tool` 重试外壳 + 单次分发 + 各 handler 分支（编排层）。
//! - `background_job`：非 `run_command` 工具的后台任务门闩与发起。
//! - `http::{gate, tools}`：出站网络闸门（含网络轴矩阵测试）与四个出站工具入口。
//! - `terminal_session`：PTY 执行。
//! - `run_command`：`run_command` 执行流水线（#1196 已先行子模块化）。
//!
//! 行数上限（`file ≤ 920`）保持不变——本模块的目的不是把分片合并成大文件，而是把
//! 「同作用域文本拼接」换成「有边界的模块」。

use std::collections::HashSet;
use std::sync::Arc;

use crate::cm_config::AgentConfig;
use crate::cm_types::ToolCall;

use super::meta::HandlerLookupTable;
use super::runtime::ToolRuntime;

/// Web UI：未选择工作区时的统一提示尾句（命令类工具共用）。
const WEB_WORKSPACE_PANEL_HINT: &str = "请先在右侧工作区面板设置目录（可选择目录或手动输入路径）。";

fn web_tool_err_workspace_not_set(action_zh: &str) -> String {
    format!("错误：未设置工作区，禁止{action_zh}。{WEB_WORKSPACE_PANEL_HINT}")
}

/// 在配置白名单基础上追加一条命令名（`run_command` 审批通过路径共用）。
fn extend_allowed_commands_arc(
    base: &std::sync::Arc<[String]>,
    cmd: &str,
) -> std::sync::Arc<[String]> {
    let mut v: Vec<String> = base.iter().cloned().collect();
    v.push(cmd.to_string());
    v.into()
}

pub struct DispatchToolCall<'a> {
    pub name: &'a str,
    pub args: &'a str,
    pub tc: &'a ToolCall,
}

pub struct DispatchToolWorkspace<'a> {
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub workspace_changelist:
        Option<std::sync::Arc<crate::cm_internal::workspace::changelist::WorkspaceChangelist>>,
}

impl<'a> DispatchToolWorkspace<'a> {
    /// 测试形态：工作区已设置、无变更清单。
    ///
    /// `tool_registry::tests` 的 6 个用例都要这一份「正常已设置工作区」上下文，
    /// 字段值逐字相同。生产侧三处构造（`tool_dispatch` / `exec_serial` /
    /// `parallel_readonly`）的字段来源互不相同，`&Path` 也没有合理默认值，
    /// 因此只提供测试入口而不 derive `Default`。
    #[cfg(test)]
    pub(crate) fn for_tests(effective_working_dir: &'a std::path::Path) -> Self {
        Self {
            effective_working_dir,
            workspace_is_set: true,
            workspace_changelist: None,
        }
    }
}

/// 配置、白名单与 handler 查找（每次 dispatch 必带）。
pub struct DispatchToolPolicy<'a> {
    pub cfg: &'a Arc<AgentConfig>,
    /// 多角色工具白名单；`None` 不限制。
    pub turn_allow: Option<&'a HashSet<String>>,
    /// 与 `crate::cm_internal::RunAgentTurnParams::obs` 的 `process_handles` 同源。
    pub handler_lookup: &'a HandlerLookupTable,
    pub sync_default_sandbox_backend: &'a Arc<dyn crate::cm_internal::tool_sandbox::SyncDefaultSandboxBackend>,
    /// 本回合权限能力快照（S3 接线：命令白名单 / 路径 / 网络 / 外部工具）。
    ///
    /// `None` = 调用方尚未接线（运维 CLI、单测、内部直调），等价于现状：只按 `cfg` 判。
    /// **不要**在这里默认造一份 `project_caps`——那会让未接线路径凭空获得一个等级，
    /// 等于把「没接线」变成「按某等级静默收窄」。
    pub permission: Option<&'a crate::cm_agent::permission::PermissionCaps>,
}

pub struct DispatchToolObs<'a> {
    pub sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    pub sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    /// 用户取消；`run_command` 等待循环会观察此标志。
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// 后台任务注册表；`None` 时 `run_command` 的 `async=true` 返回未启用（默认关闭路径）。
    pub tool_jobs: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
}

/// 只读缓存、LTM、MCP 等附属宿主（字段不删，仅分组）。
pub struct DispatchToolMemory<'a> {
    pub read_file_turn_cache:
        Option<std::sync::Arc<crate::cm_internal::read_file_turn_cache::ReadFileTurnCache>>,
    pub long_term_memory: Option<Arc<crate::cm_internal::memory::long_term_memory::LongTermMemoryRuntime>>,
    pub long_term_memory_scope_id: Option<String>,
    pub mcp_turn: Option<&'a crate::cm_internal::mcp::McpTurnHandle>,
}

/// 单次工具分发入参（嵌套分组，避免顶层继续平铺胀袋）。
pub struct DispatchToolParams<'a> {
    pub runtime: ToolRuntime<'a>,
    pub call: DispatchToolCall<'a>,
    pub workspace: DispatchToolWorkspace<'a>,
    pub policy: DispatchToolPolicy<'a>,
    pub obs: DispatchToolObs<'a>,
    pub memory: DispatchToolMemory<'a>,
}

/// [`DispatchToolParams`] 中与 Docker / 配置快照相关的字段合并，降低内部分发函数的形参个数。
struct ToolExecEnv<'a> {
    cfg: &'a Arc<AgentConfig>,
    sandbox_backend: &'a Arc<dyn crate::cm_internal::tool_sandbox::SyncDefaultSandboxBackend>,
    /// 本回合权限能力快照；`None` = 未接线（按 `cfg` 现状判定）。
    permission: Option<&'a crate::cm_agent::permission::PermissionCaps>,
}

mod background_job;
mod dispatch;
mod http;
mod run_command;
mod terminal_session;

// 显式接口（#1198）：**逐项列出**兄弟子模块之间、以及 `run_command` 对外的调用面。
// 不用 glob 重导出是有意的——glob 会把「谁依赖谁」重新藏起来，正是本 Issue 要消除的问题。
// 生产侧只有这些名字需要进入 `execute` 作用域；测试专用的项由各测试文件直接 `use`
// 对应子模块（`super::dispatch::…` / `super::background_job::…`）。
pub(in crate::cm_internal::tool_registry::execute) use background_job::{
    launch_background_job, try_dispatch_background_async_tool,
};
pub(in crate::cm_internal::tool_registry::execute) use http::http_fetch_prefetch_approval;
pub(in crate::cm_internal::tool_registry::execute) use http::{
    execute_get_weather_web, execute_http_fetch_impl, execute_http_request_impl,
    execute_web_search_web,
};
pub(in crate::cm_internal::tool_registry::execute) use terminal_session::{
    TerminalSessionExecInvoke, execute_terminal_session_impl,
};

// 这两条对 `crate` 可见：`tool_registry/mod.rs` 以 `pub use execute::{…}` 对外重导出。
pub use dispatch::{dispatch_tool, prefetch_parallel_syncdefault_approvals};
pub use run_command::prefetch_http_fetch_parallel_approvals;

#[cfg(test)]
#[path = "execute_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "execute_run_command_gate_tests.rs"]
mod run_command_gate_tests;

#[cfg(test)]
#[path = "execute_allowlist_scope_tests.rs"]
mod allowlist_scope_tests;
