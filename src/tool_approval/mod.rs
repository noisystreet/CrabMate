//! 敏感工具 **Web / CLI 人工审批** 的单一真源：按「能力等级 + 展示字段」构造请求，避免 `run_command` / `http_fetch` / `http_request` / 工作流各处重复 SSE 与终端逻辑。
//!
//! - **能力等级** [`SensitiveCapability`]：用于日志、后续策略扩展（配额、按能力关闭 `--yes` 等）；当前不单独分支行为。
//! - **策略**：Web 走 SSE `command_approval` + timeline；CLI 在 [`CliApprovalInput::auto_approve_all_sensitive`] 为真时等价「本次允许」（与历史 **`--yes`** 对齐），否则子模块 **`cli_terminal`** 的 dialoguer / 读行。
//! - **Web 通道模式**：[`WebApprovalChannelMode::Strict`] 在 `send` 失败时立即返回 Err（`tool_registry`）；[`WebApprovalChannelMode::Lenient`] 仍等待 receiver（工作流历史行为）。

mod cli_terminal;

use std::collections::HashSet;
use std::sync::Arc;

use log::debug;
use tokio::sync::{Mutex, mpsc};

use crate::types::CommandApprovalDecision;

/// 需人工确认的能力域（与 `tool_registry` 中带审批的工具对齐；后续可接配置策略）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SensitiveCapability {
    /// 宿主 shell：`run_command`（含工作流内同工具）。
    HostShell,
    /// 出站只读 HTTP：`http_fetch`。
    OutboundHttpRead,
    /// 出站可写/非常规方法：`http_request`。
    OutboundHttpWrite,
    /// 工作流 `requires_approval` 或图内 `run_command` 审批节点。
    WorkflowGate,
    /// 工作区外路径访问（如 `read_dir` 使用绝对路径或 `..` 跨越工作区边界）。
    WorkspaceExternalPath,
}

/// Web 侧审批通道句柄（与 [`crate::tool_registry::WebToolRuntime`] 字段一致，避免 `tool_approval` → `tool_registry` 依赖）。
pub struct WebApprovalSink<'a> {
    pub out_tx: &'a mpsc::Sender<String>,
    pub approval_rx_shared: &'a Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>>,
    pub approval_request_guard: &'a Arc<Mutex<()>>,
}

/// CLI 侧策略子集（**不**含 `run_command` 的 `--approve-commands`，该合并仍由各执行路径在调用本模块前完成）。
pub struct CliApprovalInput {
    /// 与 [`crate::tool_registry::CliToolRuntime::auto_approve_all_non_whitelist_run_command`] 一致：**所有**下列敏感能力在非白名单时均自动「本次允许」（仅可信环境）。
    pub auto_approve_all_sensitive: bool,
}

/// 一次交互审批的展示与 SSE 载荷（`CommandApprovalBody` 同源字段）。
#[derive(Debug, Clone)]
pub struct ApprovalRequestSpec {
    pub capability: SensitiveCapability,
    pub sse_command: String,
    pub sse_args: String,
    pub allowlist_key: Option<String>,
    pub cli_title: &'static str,
    pub cli_detail: String,
    pub web_timeline_prefix_zh: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebApprovalChannelMode {
    Strict,
    Lenient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalWebError {
    /// Web `send` 失败（Strict 模式）或既无 Web 也无 CLI 输入。
    ChannelUnavailable,
}

/// Web / CLI 会话级 **永久允许** 集合句柄（仅 `Arc<Mutex<HashSet>>` 引用，避免 `tool_approval` 依赖 `WebToolRuntime`）。
pub struct SharedAllowlistHandles<'a> {
    pub web: Option<&'a Arc<Mutex<HashSet<String>>>>,
    pub cli: Option<&'a Arc<Mutex<HashSet<String>>>>,
}

/// 白名单未命中且已走交互审批之后的结果（`AllowOnce` / `AllowAlways` 均视为已放行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractiveGateOutcome {
    Allowed,
    Denied(String),
}

fn web_timeline_detail(spec: &ApprovalRequestSpec) -> String {
    let a = spec.sse_args.trim();
    if a.is_empty() {
        spec.sse_command.clone()
    } else {
        format!("{} {}", spec.sse_command, a)
    }
}

/// 将 `key` 写入 Web 或 CLI 的 persistent allowlist（与历史「二选一」一致）。
pub async fn persist_allowlist_key(handles: &SharedAllowlistHandles<'_>, key: &str) {
    if let Some(w) = handles.web {
        w.lock().await.insert(key.to_string());
    } else if let Some(c) = handles.cli {
        c.lock().await.insert(key.to_string());
    }
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

/// 仅 Web：发送 `command_approval`、等待决策、再发 timeline（**不在** `approval_request_guard` 内发 timeline，与历史 `tool_registry` 一致）。
pub async fn run_web_tool_approval(
    sink: WebApprovalSink<'_>,
    spec: &ApprovalRequestSpec,
    sse_log_label: &'static str,
    channel_mode: WebApprovalChannelMode,
) -> Result<CommandApprovalDecision, ToolApprovalWebError> {
    debug!(
        target: "crabmate",
        "tool_approval web round capability={:?} command={} mode={:?}",
        spec.capability,
        spec.sse_command,
        channel_mode
    );
    let decision = {
        let _guard = sink.approval_request_guard.lock().await;
        let line = crate::sse::encode_message(crate::sse::SsePayload::CommandApproval {
            command_approval_request: crate::sse::CommandApprovalBody {
                command: spec.sse_command.clone(),
                args: spec.sse_args.clone(),
                allowlist_key: spec.allowlist_key.clone(),
            },
        });
        let sent = crate::sse::send_string_logged(sink.out_tx, line, sse_log_label).await;
        if matches!(channel_mode, WebApprovalChannelMode::Strict) && !sent {
            return Err(ToolApprovalWebError::ChannelUnavailable);
        }
        let mut rx_guard = sink.approval_rx_shared.lock().await;
        rx_guard
            .recv()
            .await
            .unwrap_or(CommandApprovalDecision::Deny)
    };
    let detail = web_timeline_detail(spec);
    crate::sse::web_approval::send_timeline_approval_decision(
        sink.out_tx,
        spec.web_timeline_prefix_zh,
        Some(detail),
        decision,
        "tool_approval::web_timeline",
    )
    .await;
    Ok(decision)
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
        return Ok(cli_terminal::prompt_tool_approval_cli(spec.cli_title, &spec.cli_detail).await);
    }
    Err(ToolApprovalWebError::ChannelUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_timeline_detail_empty_args() {
        let spec = ApprovalRequestSpec {
            capability: SensitiveCapability::HostShell,
            sse_command: "git".to_string(),
            sse_args: "   ".to_string(),
            allowlist_key: None,
            cli_title: "t",
            cli_detail: String::new(),
            web_timeline_prefix_zh: "p",
        };
        assert_eq!(web_timeline_detail(&spec), "git");
    }

    #[test]
    fn web_timeline_detail_with_args() {
        let spec = ApprovalRequestSpec {
            capability: SensitiveCapability::OutboundHttpRead,
            sse_command: "http_fetch".to_string(),
            sse_args: "GET https://a/".to_string(),
            allowlist_key: None,
            cli_title: "t",
            cli_detail: String::new(),
            web_timeline_prefix_zh: "p",
        };
        assert_eq!(web_timeline_detail(&spec), "http_fetch GET https://a/");
    }
}
