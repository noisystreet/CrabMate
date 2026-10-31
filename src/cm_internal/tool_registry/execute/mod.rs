//! `dispatch_tool` 及各类需异步/阻塞池的工具执行实现。
//!
//! 进入阻塞池前对 [`AgentConfig`] 使用 [`Arc::clone`]；白名单等同理。详见仓库 `tool_registry` 模块说明。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use log::error;

use crate::cm_internal::tool_approval::{self, InteractiveGateOutcome};
use crate::cm_internal::tools;
use crate::cm_config::{AgentConfig, SyncDefaultToolSandboxMode};
use crate::cm_tools::tool_retry_policy::ToolRetrySpec;
use crate::cm_types::{CommandApprovalDecision, ToolCall};

use super::meta::{HandlerId, HandlerLookupTable};
use super::policy::{
    http_fetch_outer_wall_secs, http_request_outer_wall_secs, parallel_tool_wall_timeout_secs,
    sync_default_runs_inline, terminal_session_outer_wall_secs, web_search_outer_wall_secs,
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

impl<'a> DispatchToolWorkspace<'a> {
    /// 测试形态：工作区已设置、无变更清单。
    ///
    /// `tool_registry::tests` 的 6 个用例都要这一份「正常已设置工作区」上下文，
    /// 字段值逐字相同。生产侧三处构造（`tool_dispatch` / `exec_serial` /
    /// `parallel_readonly`）的字段来源互不相同，`&Path` 也没有合理默认值，
    /// 因此只提供测试入口而不 derive `Default`。
    #[cfg(test)]
    pub(crate) fn for_tests(effective_working_dir: &'a Path) -> Self {
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
    /// 后台任务注册表：供 `background_job_status` / `background_job_list` 只读查询。
    tool_jobs: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
}

/// [`DispatchToolParams`] 中与 Docker / 配置快照相关的字段合并，降低内部分发函数的形参个数。
struct ToolExecEnv<'a> {
    cfg: &'a Arc<AgentConfig>,
    sandbox_backend: &'a Arc<dyn crate::cm_internal::tool_sandbox::SyncDefaultSandboxBackend>,
    /// 本回合权限能力快照；`None` = 未接线（按 `cfg` 现状判定）。
    permission: Option<&'a crate::cm_agent::permission::PermissionCaps>,
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
    permission: Option<&crate::cm_agent::permission::PermissionCaps>,
) -> Result<(), String> {
    let Some(ext_path) = read_dir_path_is_external(args) else {
        return Ok(());
    };
    // S3：路径轴先定论——L4 放行（不弹窗），L2 硬拒（不弹窗），其余走现状审批。
    if let Some(caps) = permission {
        // 审计正文只留末两段路径（完整绝对路径不进日志）。
        let audit_detail = crate::cm_tools::tools::audit_path_detail(&ext_path);
        if caps.path_scope.is_unrestricted() {
            crate::cm_agent::permission_audit::record_allowed(
                caps,
                crate::cm_agent::permission_audit::PermissionAxis::Path,
                Some("read_dir"),
                Some(&audit_detail),
            );
            return Ok(());
        }
        if caps.approval == crate::cm_agent::permission::ApprovalPolicy::Deny {
            crate::cm_agent::permission_audit::record_denied(
                caps,
                crate::cm_agent::permission_audit::PermissionAxis::Path,
                Some("read_dir"),
                Some(&audit_detail),
            );
            return Err(format!(
                "错误：当前权限等级（{}）下不允许 read_dir 访问工作区外路径 \"{}\"：{}。",
                caps.level.as_str(),
                ext_path,
                caps.level.promise_zh()
            ));
        }
    }
    if web_ctx.is_none() {
        return Err(format!(
            "错误：read_dir 访问工作区外路径 \"{}\" 需要审批通道（当前无可用会话）。",
            ext_path
        ));
    }
    let spec = tool_approval::approval_spec_read_dir_external_path(&ext_path);
    let allow_handles =
        tool_approval::shared_allowlist_handles_web(web_ctx, permission.map(|c| c.level));
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
        Err(e) => Err(e.user_message_zh().to_string()),
    }
}

include!("execute_dispatch_body.inc.rs");
include!("execute_background_tool_async.inc.rs");
include!("execute_terminal_session.inc.rs");
include!("http_tool_exec.inc.rs");
include!("execute_http_tools.inc.rs");

/// `run_command` 的执行流水线：原为 7 个 `include!` 分片，现为独立子模块（#1196）。
mod run_command;

// 子模块中 `pub(super)` 的项在这里以**裸名**进入 `execute`：其余 `include!` 分片与
// `tests` 继续沿用原调用点，无需逐个改路径。
use run_command::*;
// `tool_registry/mod.rs` 用 `pub use execute::{…}` 对外重导出这一条，故必须保持 `pub`。
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
