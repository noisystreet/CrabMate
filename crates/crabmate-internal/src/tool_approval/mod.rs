//! 敏感工具 **Web 人工审批** 的组装层：Web SSE 往返见 [`crabmate_approval`]。
//!
//! - **能力等级** [`SensitiveCapability`]：用于日志、后续策略扩展；当前不单独分支行为。
//! - **策略**：Web 走 SSE `command_approval` + timeline；无 Web 通道时返回
//!   [`ToolApprovalWebError::ChannelUnavailable`]（运维 CLI 无同进程终端审批；官方对话用 Client / Web）。
//! - **Web 通道模式**：见 [`WebApprovalChannelMode`]。

use log::debug;

pub use crabmate_approval::{
    ApprovalRequestSpec, InteractiveGateOutcome, SensitiveCapability, SharedAllowlistHandles,
    ToolApprovalWebError, WebApprovalChannelMode, WebApprovalSink, persist_allowlist_key,
    run_web_tool_approval,
};
use crabmate_types::CommandApprovalDecision;

/// 从 [`crate::tool_registry::WebToolRuntime`] 构造 Web 审批通道（类型定义在 `crabmate-tools`）。
pub fn web_tool_runtime_approval_sink(
    rt: &crabmate_tools::tool_runtime::WebToolRuntime,
) -> WebApprovalSink<'_> {
    WebApprovalSink {
        out_tx: &rt.out_tx,
        approval_rx_shared: &rt.approval_rx_shared,
        approval_request_guard: &rt.approval_request_guard,
    }
}

/// 配置白名单与 persistent 集合均未放行时：发起交互审批，并在 `AllowAlways` 时写入 [`ApprovalRequestSpec::allowlist_key`]（若有）。
pub async fn interactive_gate_after_whitelist_miss(
    web: Option<WebApprovalSink<'_>>,
    spec: &ApprovalRequestSpec,
    sse_log_label: &'static str,
    allowlist: &SharedAllowlistHandles<'_>,
) -> Result<InteractiveGateOutcome, ToolApprovalWebError> {
    let decision = request_tool_interactive_approval(web, spec, sse_log_label).await?;
    match decision {
        CommandApprovalDecision::Deny => Ok(InteractiveGateOutcome::Denied(format!(
            "用户拒绝 {}：{}",
            spec.sse_command,
            spec.sse_args.trim()
        ))),
        CommandApprovalDecision::AllowOnce => Ok(InteractiveGateOutcome::Allowed),
        CommandApprovalDecision::AllowAlways => {
            if let Some(k) = spec.allowlist_key.as_deref() {
                persist_allowlist_key(allowlist, k).await;
            }
            Ok(InteractiveGateOutcome::Allowed)
        }
    }
}

/// 仅 Web SSE 审批；无通道则 [`ToolApprovalWebError::ChannelUnavailable`]。
pub async fn request_tool_interactive_approval(
    web: Option<WebApprovalSink<'_>>,
    spec: &ApprovalRequestSpec,
    sse_log_label: &'static str,
) -> Result<CommandApprovalDecision, ToolApprovalWebError> {
    if let Some(sink) = web {
        debug!(
            target: "crabmate",
            "tool_approval web capability={:?} title={}",
            spec.capability,
            spec.cli_title
        );
        return run_web_tool_approval(sink, spec, sse_log_label, WebApprovalChannelMode::Strict)
            .await;
    }
    Err(ToolApprovalWebError::ChannelUnavailable)
}
