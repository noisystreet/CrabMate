//! 敏感工具 **Web / CLI 人工审批** 的组装层：Web SSE 往返见 [`crabmate_approval`]；
//! CLI / TUI 终端与 dialoguer 仍在本模块。
//!
//! - **能力等级** [`SensitiveCapability`]：用于日志、后续策略扩展；当前不单独分支行为。
//! - **策略**：Web 走 SSE `command_approval` + timeline；CLI 在 [`CliApprovalInput::auto_approve_all_sensitive`] 为真时等价「本次允许」（与历史 **`--yes`** 对齐），否则子模块 **`cli_terminal`** 的 dialoguer / 读行。
//! - **Web 通道模式**：见 [`WebApprovalChannelMode`]。

mod cli_terminal;

use log::debug;

pub use crabmate_approval::{
    ApprovalRequestSpec, InteractiveGateOutcome, SensitiveCapability, SharedAllowlistHandles,
    ToolApprovalWebError, WebApprovalChannelMode, WebApprovalSink, persist_allowlist_key,
    run_web_tool_approval,
};
pub use crabmate_tools::tool_runtime::TuiApprovalRequest;
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

/// CLI 侧策略子集（**不**含 `run_command` 的 `--approve-commands`，该合并仍由各执行路径在调用本模块前完成）。
pub struct CliApprovalInput {
    /// 与 [`crate::tool_registry::CliToolRuntime::auto_approve_all_non_whitelist_run_command`] 一致：**所有**下列敏感能力在非白名单时均自动「本次允许」（仅可信环境）。
    pub auto_approve_all_sensitive: bool,
    /// `crabmate tui`：发往 UI 线程的阻塞审批队列（容量小，`send` 可能阻塞）。
    pub tui_blocking_approval_tx: Option<std::sync::mpsc::SyncSender<TuiApprovalRequest>>,
}

/// 配置白名单与 persistent 集合均未放行时：发起交互审批，并在 `AllowAlways` 时写入 [`ApprovalRequestSpec::allowlist_key`]（若有）。
pub async fn interactive_gate_after_whitelist_miss(
    web: Option<WebApprovalSink<'_>>,
    cli: Option<CliApprovalInput>,
    spec: &ApprovalRequestSpec,
    sse_log_label: &'static str,
    allowlist: &SharedAllowlistHandles<'_>,
) -> Result<InteractiveGateOutcome, ToolApprovalWebError> {
    let decision = request_tool_interactive_approval(web, cli, spec, sse_log_label).await?;
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

/// Web 优先，否则 CLI；均无则 [`ToolApprovalWebError::ChannelUnavailable`]。
pub async fn request_tool_interactive_approval(
    web: Option<WebApprovalSink<'_>>,
    cli: Option<CliApprovalInput>,
    spec: &ApprovalRequestSpec,
    sse_log_label: &'static str,
) -> Result<CommandApprovalDecision, ToolApprovalWebError> {
    if let Some(sink) = web {
        return run_web_tool_approval(sink, spec, sse_log_label, WebApprovalChannelMode::Strict)
            .await;
    }
    if let Some(cli_in) = cli {
        if cli_in.auto_approve_all_sensitive {
            debug!(
                target: "crabmate",
                "tool_approval cli auto_approve capability={:?} title={}",
                spec.capability,
                spec.cli_title
            );
            return Ok(CommandApprovalDecision::AllowOnce);
        }
        if let Some(ref tx) = cli_in.tui_blocking_approval_tx {
            let tx = tx.clone();
            let title = spec.cli_title.to_string();
            let detail = spec.cli_detail.clone();
            return Ok(tokio::task::spawn_blocking(move || {
                let (respond_tx, respond_rx) = std::sync::mpsc::channel();
                let req = TuiApprovalRequest {
                    title,
                    detail,
                    respond_tx,
                };
                if tx.send(req).is_err() {
                    return CommandApprovalDecision::Deny;
                }
                respond_rx.recv().unwrap_or(CommandApprovalDecision::Deny)
            })
            .await
            .unwrap_or(CommandApprovalDecision::Deny));
        }
        return Ok(cli_terminal::prompt_tool_approval_cli(spec.cli_title, &spec.cli_detail).await);
    }
    Err(ToolApprovalWebError::ChannelUnavailable)
}
