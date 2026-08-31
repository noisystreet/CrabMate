//! `dispatch_tool` 及各类需异步/阻塞池的工具执行实现。
//!
//! 进入阻塞池前对 [`AgentConfig`] 使用 [`Arc::clone`]；白名单等同理。详见仓库 `tool_registry` 模块说明。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use log::error;

use crate::cm_internal::tool_approval::{
    self, InteractiveGateOutcome, ToolApprovalWebError,
};
use crate::cm_internal::tools;
use crate::cm_config::{AgentConfig, SyncDefaultToolSandboxMode};
use crate::cm_tools::tool_retry_policy::ToolRetrySpec;
use crate::cm_types::{CommandApprovalDecision, ToolCall};

use super::meta::{HandlerId, HandlerLookupTable};
use super::policy::{
    http_fetch_outer_wall_secs, http_request_outer_wall_secs, parallel_tool_wall_timeout_secs,
    sync_default_runs_inline, web_search_outer_wall_secs,
};
use super::runtime::{ToolRuntime, WebToolRuntime};

/// Web UI：未选择工作区时的统一提示尾句（`run_command` / `run_executable` 共用）。
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
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub workspace_changelist:
        Option<std::sync::Arc<crate::cm_internal::workspace::changelist::WorkspaceChangelist>>,
}

/// 配置、白名单与 handler 查找（每次 dispatch 必带）。
pub struct DispatchToolPolicy<'a> {
    pub cfg: &'a Arc<AgentConfig>,
    /// 多角色工具白名单；`None` 不限制。
    pub turn_allow: Option<&'a HashSet<String>>,
    /// 与 [`crate::cm_internal::RunAgentTurnParams::obs`] 的 `process_handles` 同源。
    pub handler_lookup: &'a HandlerLookupTable,
    pub sync_default_sandbox_backend: &'a Arc<dyn crate::cm_internal::tool_sandbox::SyncDefaultSandboxBackend>,
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

/// `HandlerId::SyncDefault` 分支入参（与 [`DispatchToolParams`] 中部分字段一致，避免 `dispatch_sync_default` 形参过多）。
struct SyncDefaultToolDispatchArgs<'a> {
    env: &'a ToolExecEnv<'a>,
    runtime: ToolRuntime<'a>,
    cfg: &'a Arc<AgentConfig>,
    effective_working_dir: &'a std::path::Path,
    workspace_is_set: bool,
    name: &'a str,
    args: &'a str,
    tc: &'a ToolCall,
    read_file_turn_cache: Option<std::sync::Arc<crate::cm_internal::read_file_turn_cache::ReadFileTurnCache>>,
    workspace_changelist: Option<std::sync::Arc<crate::cm_internal::workspace::changelist::WorkspaceChangelist>>,
    long_term_memory: Option<Arc<crate::cm_internal::memory::long_term_memory::LongTermMemoryRuntime>>,
    long_term_memory_scope_id: Option<String>,
}

/// [`DispatchToolParams`] 中与 Docker / 配置快照相关的字段合并，降低内部分发函数的形参个数。
struct ToolExecEnv<'a> {
    cfg: &'a Arc<AgentConfig>,
    sandbox_backend: &'a Arc<dyn crate::cm_internal::tool_sandbox::SyncDefaultSandboxBackend>,
}

/// `http_fetch` / `http_request` 共用：可选 Web 审批会话（本路径不使用 `workspace_changed`）。
fn http_tool_approval_context<'a>(runtime: ToolRuntime<'a>) -> Option<&'a WebToolRuntime> {
    runtime.ctx
}

/// 检测 `read_dir` 入参中 `path` 是否为外部路径（绝对路径或含 `..`）。
fn read_dir_path_is_external(args_json: &str) -> Option<String> {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let path = v.get("path")?.as_str()?.trim();
    if path.starts_with('/') || path.contains("..") {
        return Some(path.to_string());
    }
    None
}

async fn approve_external_read_dir_if_needed(
    args: &str,
    web_ctx: Option<&WebToolRuntime>,
) -> Result<(), String> {
    let Some(ext_path) = read_dir_path_is_external(args) else {
        return Ok(());
    };
    if web_ctx.is_none() {
        return Err(format!(
            "错误：read_dir 访问工作区外路径 \"{}\" 需要审批通道（当前无可用会话）。",
            ext_path
        ));
    }
    let spec = tool_approval::approval_spec_read_dir_external_path(&ext_path);
    let allow_handles = tool_approval::shared_allowlist_handles_web(web_ctx);
    match tool_approval::interactive_gate_after_whitelist_miss(
        web_ctx.map(tool_approval::web_tool_runtime_approval_sink),
        &spec,
        "tool_registry::read_dir external path approval",
        &allow_handles,
    )
    .await
    {
        Ok(InteractiveGateOutcome::Allowed) => Ok(()),
        Ok(InteractiveGateOutcome::Denied(msg)) => Err(format!("已拒绝：{}", msg)),
        Err(ToolApprovalWebError::ChannelUnavailable) => {
            Err(tool_approval::INTERACTIVE_GATE_CHANNEL_UNAVAILABLE_ERR.to_string())
        }
    }
}

include!("execute_dispatch_body.inc.rs");
include!("execute_run_command_docker.inc.rs");
include!("execute_run_command_approval.inc.rs");
include!("execute_run_command_chunks.inc.rs");
include!("execute_run_command_async.inc.rs");
include!("execute_run_command_host.inc.rs");
include!("execute_run_command_sync.inc.rs");
include!("execute_terminal_session.inc.rs");
include!("http_tool_exec.inc.rs");
include!("execute_http_tools.inc.rs");

#[cfg(test)]
#[path = "execute_tests.rs"]
mod tests;
