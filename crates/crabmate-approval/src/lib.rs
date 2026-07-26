//! 敏感工具 **Web 人工审批** 的类型与 SSE 往返（不含 CLI/TUI 终端实现）。
//!
//! - **能力等级** [`SensitiveCapability`]：日志与策略扩展点。
//! - **Web 通道模式**：[`WebApprovalChannelMode::Strict`] 在 `send` 失败时立即 Err；
//!   [`WebApprovalChannelMode::Lenient`] 仍等待 receiver（工作流历史行为）。
//!
//! CLI / dialoguer / TUI 阻塞队列见 `crabmate-internal::tool_approval`。

use std::collections::HashSet;
use std::sync::Arc;

use crabmate_types::CommandApprovalDecision;
use log::debug;
use tokio::sync::{Mutex, mpsc};

/// 需人工确认的能力域（与带审批的工具对齐；后续可接配置策略）。
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

/// Web 侧审批通道句柄（与 `WebToolRuntime` 字段一致，避免本 crate 依赖 tool_registry）。
pub struct WebApprovalSink<'a> {
    pub out_tx: &'a mpsc::Sender<String>,
    pub approval_rx_shared: &'a Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>>,
    pub approval_request_guard: &'a Arc<Mutex<()>>,
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

/// Web / CLI 会话级 **永久允许** 集合句柄。
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

pub(crate) fn web_timeline_detail(spec: &ApprovalRequestSpec) -> String {
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

/// 仅 Web：发送 `command_approval`、等待决策、再发 timeline（**不在** `approval_request_guard` 内发 timeline）。
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
        let line = crabmate_sse_protocol::sse::encode_message(
            crabmate_sse_protocol::sse::SsePayload::CommandApproval {
                command_approval_request: crabmate_sse_protocol::sse::CommandApprovalBody {
                    command: spec.sse_command.clone(),
                    args: spec.sse_args.clone(),
                    allowlist_key: spec.allowlist_key.clone(),
                },
            },
        );
        let sent =
            crabmate_sse_protocol::sse::send_string_logged(sink.out_tx, line, sse_log_label).await;
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
    crabmate_sse_protocol::sse::web_approval::send_timeline_approval_decision(
        sink.out_tx,
        spec.web_timeline_prefix_zh,
        Some(detail),
        decision,
        "tool_approval::web_timeline",
    )
    .await;
    Ok(decision)
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
