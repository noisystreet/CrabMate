//! 敏感工具 **Web 人工审批** 的组装层：Web SSE 往返见 `crate::cm_approval`。
//!
//! - **能力等级** [`SensitiveCapability`]：用于日志、后续策略扩展；当前不单独分支行为。
//! - **策略**：Web 走 SSE `command_approval` + timeline；无 Web 通道时返回
//!   [`ToolApprovalWebError::ChannelUnavailable`]（运维 CLI 无同进程终端审批；官方对话用 Client / Web）。
//! - **Web 通道模式**：见 [`WebApprovalChannelMode`]。

use log::debug;

mod approval_specs;

pub use approval_specs::{
    http_fetch as approval_spec_http_fetch,
    http_request as approval_spec_http_request,
    read_dir_external_path as approval_spec_read_dir_external_path,
    run_command_unknown_cmd as approval_spec_run_command_unknown_cmd,
    shell_script as approval_spec_shell_script,
    workspace_external_path as approval_spec_workspace_external_path,
};
pub use crate::cm_approval::{
    ApprovalRequestSpec, InteractiveGateOutcome, SensitiveCapability, SharedAllowlistHandles,
    ToolApprovalWebError, WebApprovalChannelMode, WebApprovalSink, persist_allowlist_key,
    run_web_tool_approval,
};
use crate::cm_types::CommandApprovalDecision;

/// 从 [`crate::cm_internal::tool_registry::WebToolRuntime`] 构造 Web 审批通道（类型定义在 `cm_tools`）。
pub fn web_tool_runtime_approval_sink(
    rt: &crate::cm_tools::tool_runtime::WebToolRuntime,
) -> WebApprovalSink<'_> {
    WebApprovalSink {
        out_tx: &rt.out_tx,
        approval_rx_shared: &rt.approval_rx_shared,
        approval_request_guard: &rt.approval_request_guard,
        cancel: rt.cancel.as_ref(),
    }
}

/// 审批能力域 → 审计轴（设计 §11.2）。
///
/// `WorkflowGate` 是节点级的跨轴放行（可能是命令、也可能是任意工具），归 `Tool`：
/// 审计轴要能一句话解释「越界发生在哪一类能力上」，含糊时就报最外层的工具轴。
fn audit_axis_for(
    capability: crate::cm_approval::SensitiveCapability,
) -> crate::cm_agent::permission_audit::PermissionAxis {
    use crate::cm_agent::permission_audit::PermissionAxis;
    use crate::cm_approval::SensitiveCapability;
    match capability {
        SensitiveCapability::HostShell => PermissionAxis::Command,
        SensitiveCapability::OutboundHttpRead | SensitiveCapability::OutboundHttpWrite => {
            PermissionAxis::Net
        }
        SensitiveCapability::WorkspaceExternalPath => PermissionAxis::Path,
        SensitiveCapability::WorkflowGate => PermissionAxis::Tool,
    }
}

/// 记一条「人批准了这次越界」。`level = None`（未接线等级的旧路径）不记。
fn audit_approval_granted(spec: &ApprovalRequestSpec, level: Option<crate::cm_types::PermissionLevel>) {
    let Some(level) = level else {
        return;
    };
    // 只带 `sse_command`，不带 `sse_args`：后者可能含路径、query 等细节。
    crate::cm_agent::permission_audit::record_approved(
        level,
        audit_axis_for(spec.capability),
        None,
        Some(spec.sse_command.as_str()),
    );
}

/// 配置白名单与 persistent 集合均未放行时：发起交互审批，并在 `AllowAlways` 时写入 [`ApprovalRequestSpec::allowlist_key`]（若有）。
pub async fn interactive_gate_after_whitelist_miss(
    web: Option<WebApprovalSink<'_>>,
    spec: &ApprovalRequestSpec,
    sse_log_label: &'static str,
    allowlist: &SharedAllowlistHandles<'_>,
) -> Result<InteractiveGateOutcome, ToolApprovalWebError> {
    let decision = request_tool_interactive_approval(web, spec, sse_log_label, allowlist.level).await?;
    match decision {
        CommandApprovalDecision::Deny => Ok(InteractiveGateOutcome::Denied(format!(
            "用户拒绝 {}：{}",
            spec.sse_command,
            spec.sse_args.trim()
        ))),
        CommandApprovalDecision::AllowOnce => {
            audit_approval_granted(spec, allowlist.level);
            Ok(InteractiveGateOutcome::Allowed)
        }
        CommandApprovalDecision::AllowAlways => {
            if let Some(k) = spec.allowlist_key.as_deref() {
                persist_allowlist_key(allowlist, k).await;
            }
            audit_approval_granted(spec, allowlist.level);
            Ok(InteractiveGateOutcome::Allowed)
        }
    }
}

/// 仅 Web SSE 审批；无通道则 [`ToolApprovalWebError::ChannelUnavailable`]。
///
/// `permission_level` 只用于 SSE 载荷展示（设计 §16.5），不参与任何放行判断——
/// 判据仍在 `cm_agent::permission`，避免审批层长出第二套等级比较。
pub async fn request_tool_interactive_approval(
    web: Option<WebApprovalSink<'_>>,
    spec: &ApprovalRequestSpec,
    sse_log_label: &'static str,
    permission_level: Option<crate::cm_types::PermissionLevel>,
) -> Result<CommandApprovalDecision, ToolApprovalWebError> {
    if let Some(sink) = web {
        debug!(
            target: "crabmate",
            "tool_approval web capability={:?} title={}",
            spec.capability,
            spec.cli_title
        );
        return run_web_tool_approval(
            sink,
            spec,
            sse_log_label,
            WebApprovalChannelMode::Strict,
            permission_level,
        )
        .await;
    }
    Err(ToolApprovalWebError::ChannelUnavailable)
}

/// 从 [`crate::cm_tools::tool_runtime::WebToolRuntime`] 构造 persistent allowlist 句柄。
///
/// `level` 是本回合生效等级，用于给 `AllowAlways` 的键加**作用域前缀**（设计 §6.3）：
/// 只传 `Option` 而不是 `&PermissionCaps`，是因为本层只需要「键写进哪一档」这一件事。
pub fn shared_allowlist_handles_web(
    web_ctx: Option<&crate::cm_tools::tool_runtime::WebToolRuntime>,
    level: Option<crate::cm_types::PermissionLevel>,
) -> SharedAllowlistHandles<'_> {
    SharedAllowlistHandles {
        web: web_ctx.map(|w| &w.persistent_allowlist_shared),
        level,
    }
}

/// Web 会话下的交互审批门控（封装 allowlist 句柄与 sink 构造）。
pub async fn interactive_gate_web_runtime(
    web_ctx: Option<&crate::cm_tools::tool_runtime::WebToolRuntime>,
    level: Option<crate::cm_types::PermissionLevel>,
    spec: &ApprovalRequestSpec,
    sse_log_label: &'static str,
) -> Result<InteractiveGateOutcome, ToolApprovalWebError> {
    interactive_gate_after_whitelist_miss(
        web_ctx.map(web_tool_runtime_approval_sink),
        spec,
        sse_log_label,
        &shared_allowlist_handles_web(web_ctx, level),
    )
    .await
}

/// 将交互门控结果映射为工具层错误串（允许 / 拒绝 / 通道不可用）。
pub fn interactive_gate_outcome_to_tool_err(outcome: InteractiveGateOutcome) -> Result<(), String> {
    match outcome {
        InteractiveGateOutcome::Allowed => Ok(()),
        InteractiveGateOutcome::Denied(msg) => Err(msg),
    }
}

/// HTTP 工具：URL 未匹配配置前缀且无 Web 审批通道时的统一错误。
pub const HTTP_TOOL_NO_APPROVAL_CHANNEL_ERR: &str =
    "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。";

/// 交互门控失败时的统一错误文案（拒绝或通道不可用）。
pub const INTERACTIVE_GATE_CHANNEL_UNAVAILABLE_ERR: &str = "错误：审批通道不可用，请重试。";
