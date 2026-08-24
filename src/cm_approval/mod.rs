//! 敏感工具 **Web 人工审批** 的类型与 SSE 往返（不含 CLI/TUI 终端实现）。
//!
//! - **能力等级** [`SensitiveCapability`]：日志与策略扩展点。
//! - **Web 通道模式**：[`WebApprovalChannelMode::Strict`] 在 `send` 失败时立即 Err；
//!   [`WebApprovalChannelMode::Lenient`] 仍等待 receiver（工作流历史行为）。
//!
//! 运维 CLI 无同进程终端审批；官方对话审批仅 Web SSE（经 `crabmate-internal::tool_approval` 组装）。

use std::collections::HashSet;
use std::sync::Arc;

use crate::cm_types::CommandApprovalDecision;
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
    /// Web `send` 失败（Strict 模式）或无 Web 审批通道。
    ChannelUnavailable,
}

/// Web 会话级 **永久允许** 集合句柄。
pub struct SharedAllowlistHandles<'a> {
    pub web: Option<&'a Arc<Mutex<HashSet<String>>>>,
}

/// 白名单未命中且已走交互审批之后的结果（`AllowOnce` / `AllowAlways` 均视为已放行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractiveGateOutcome {
    Allowed,
    Denied(String),
}

pub(crate) fn web_timeline_detail(spec: &ApprovalRequestSpec) -> String {
    let command = spec.sse_command.trim();
    let args = spec.sse_args.trim();
    if args.is_empty() {
        return command.to_string();
    }
    if args_already_include_command(command, args) {
        return args.to_string();
    }
    format!("{command} {args}")
}

fn args_already_include_command(command: &str, args: &str) -> bool {
    !command.is_empty() && (args == command || args.starts_with(&format!("{command} ")))
}

/// 将 `key` 写入 Web persistent allowlist（无 `web` 句柄时为 no-op）。
pub async fn persist_allowlist_key(handles: &SharedAllowlistHandles<'_>, key: &str) {
    if let Some(w) = handles.web {
        w.lock().await.insert(key.to_string());
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
        let line = crate::cm_sse_protocol::sse::encode_message(
            crate::cm_sse_protocol::sse::SsePayload::CommandApproval {
                command_approval_request: crate::cm_sse_protocol::sse::CommandApprovalBody {
                    command: spec.sse_command.clone(),
                    args: spec.sse_args.clone(),
                    allowlist_key: spec.allowlist_key.clone(),
                },
            },
        );
        let sent =
            crate::cm_sse_protocol::sse::send_string_logged(sink.out_tx, line, sse_log_label).await;
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
    crate::cm_sse_protocol::sse::web_approval::send_timeline_approval_decision(
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

    #[test]
    fn web_timeline_detail_does_not_reduplicate_argv0() {
        let spec = ApprovalRequestSpec {
            capability: SensitiveCapability::HostShell,
            sse_command: "curl".to_string(),
            sse_args: "curl -s -L https://example.com".to_string(),
            allowlist_key: None,
            cli_title: "t",
            cli_detail: String::new(),
            web_timeline_prefix_zh: "p",
        };
        assert_eq!(
            web_timeline_detail(&spec),
            "curl -s -L https://example.com"
        );
    }

    #[test]
    fn web_timeline_detail_argv_tail_joins_once() {
        let spec = ApprovalRequestSpec {
            capability: SensitiveCapability::HostShell,
            sse_command: "curl".to_string(),
            sse_args: "-s -L https://example.com".to_string(),
            allowlist_key: None,
            cli_title: "t",
            cli_detail: String::new(),
            web_timeline_prefix_zh: "p",
        };
        assert_eq!(
            web_timeline_detail(&spec),
            "curl -s -L https://example.com"
        );
    }
}
