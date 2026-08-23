//! 敏感工具 **Web 人工审批** 的组装层：Web SSE 往返见 [`crate::cm_approval`]。
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

/// 从 [`crate::cm_internal::tool_registry::WebToolRuntime`] 构造 Web 审批通道（类型定义在 `crabmate-tools`）。
pub fn web_tool_runtime_approval_sink(
    rt: &crate::cm_tools::tool_runtime::WebToolRuntime,
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

/// 从 [`crate::cm_tools::tool_runtime::WebToolRuntime`] 构造 persistent allowlist 句柄。
pub fn shared_allowlist_handles_web(
    web_ctx: Option<&crate::cm_tools::tool_runtime::WebToolRuntime>,
) -> SharedAllowlistHandles<'_> {
    SharedAllowlistHandles {
        web: web_ctx.map(|w| &w.persistent_allowlist_shared),
    }
}

/// Web 会话下的交互审批门控（封装 allowlist 句柄与 sink 构造）。
pub async fn interactive_gate_web_runtime(
    web_ctx: Option<&crate::cm_tools::tool_runtime::WebToolRuntime>,
    spec: &ApprovalRequestSpec,
    sse_log_label: &'static str,
) -> Result<InteractiveGateOutcome, ToolApprovalWebError> {
    interactive_gate_after_whitelist_miss(
        web_ctx.map(web_tool_runtime_approval_sink),
        spec,
        sse_log_label,
        &shared_allowlist_handles_web(web_ctx),
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
