//! 敏感工具 **Web 人工审批** 的类型与 SSE 往返（不含 CLI/TUI 终端实现）。
//!
//! - **能力等级** [`SensitiveCapability`]：日志与策略扩展点。
//! - **Web 通道模式**：[`WebApprovalChannelMode::Strict`] 在 `send` 失败时立即 Err；
//!   [`WebApprovalChannelMode::Lenient`] 仍等待 receiver（工作流历史行为）。
//!
//! 运维 CLI 无同进程终端审批；官方对话审批仅 Web SSE（经 `cm_internal::tool_approval` 组装）。

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::cm_types::CommandApprovalDecision;
use log::debug;
use tokio::sync::{Mutex, mpsc};

/// 交互审批等待上限（秒）：超时按「拒绝」收敛，避免任务永久挂起、占死队列并发槽。
const APPROVAL_WAIT_TIMEOUT_SECS: u64 = 300;
const APPROVAL_WAIT_TIMEOUT: Duration = Duration::from_secs(APPROVAL_WAIT_TIMEOUT_SECS);
/// 取消标志（`AtomicBool`）无异步通知，只能按此间隔轮询。
const APPROVAL_CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(200);

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
    /// 当前回合的协作式取消标志；置位后审批等待提前退出。非 Web / 无取消来源时为 `None`。
    pub cancel: Option<&'a Arc<AtomicBool>>,
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
    /// 等待审批期间用户取消（例如前端点「停止」）。
    Cancelled,
    /// 等待审批超时（见 [`APPROVAL_WAIT_TIMEOUT`]），按「拒绝」处理。
    TimedOut,
}

/// 无 Web 审批通道时的统一提示（Web 会话请带上 `approval_session_id`）。
pub const TOOL_APPROVAL_CHANNEL_UNAVAILABLE_ERR: &str = "错误：审批通道不可用，请重试。";
/// 审批等待期间任务被取消。
pub const TOOL_APPROVAL_CANCELLED_ERR: &str = "错误：审批等待期间任务已被取消，未执行。";

impl ToolApprovalWebError {
    /// 面向工具结果与日志的中文提示（按错误类型区分）。
    ///
    /// 超时文案由 [`APPROVAL_WAIT_TIMEOUT_SECS`] 动态生成，避免与实际超时值漂移。
    pub fn user_message_zh(self) -> std::borrow::Cow<'static, str> {
        use std::borrow::Cow;
        match self {
            Self::ChannelUnavailable => Cow::Borrowed(TOOL_APPROVAL_CHANNEL_UNAVAILABLE_ERR),
            Self::Cancelled => Cow::Borrowed(TOOL_APPROVAL_CANCELLED_ERR),
            Self::TimedOut => Cow::Owned(format!(
                "错误：等待命令审批超时（{APPROVAL_WAIT_TIMEOUT_SECS} 秒）未获决定，已按拒绝处理。"
            )),
        }
    }
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
    format!("{command} {args}")
}

/// 将 `key` 写入 Web persistent allowlist（无 `web` 句柄时为 no-op）。
pub async fn persist_allowlist_key(handles: &SharedAllowlistHandles<'_>, key: &str) {
    if let Some(w) = handles.web {
        w.lock().await.insert(key.to_string());
    }
}

/// 取消标志已置位？
fn is_cancelled(cancel: Option<&Arc<AtomicBool>>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::SeqCst))
}

/// 丢弃通道内残留的决定：取消/超时后客户端可能才补发决定，须避免被下一次审批误用。
fn drain_late_decisions(rx: &mut mpsc::Receiver<CommandApprovalDecision>) {
    while rx.try_recv().is_ok() {}
}

/// 等待一次审批决定：同时观察 per-turn 取消标志与总超时，避免永久挂起。
///
/// - 收到决定 → 返回该决定；通道被 drop（`recv()` 为 `None`）→ 按「拒绝」收敛；
/// - 用户取消 / 超时 → 返回对应错误，调用方据此提前结束工具调用（随后主循环的
///   取消检查会终止整回合并释放队列并发槽）。
async fn await_approval_decision(
    rx: &mut mpsc::Receiver<CommandApprovalDecision>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<CommandApprovalDecision, ToolApprovalWebError> {
    // 先判取消：`select!` 为 `biased`，否则通道内残留的**迟到决定**会压过取消分支。
    if is_cancelled(cancel) {
        drain_late_decisions(rx);
        return Err(ToolApprovalWebError::Cancelled);
    }
    let outcome = tokio::select! {
        biased;
        msg = rx.recv() => Ok(msg.unwrap_or(CommandApprovalDecision::Deny)),
        _ = wait_for_cancel(cancel) => Err(ToolApprovalWebError::Cancelled),
        _ = tokio::time::sleep(APPROVAL_WAIT_TIMEOUT) => Err(ToolApprovalWebError::TimedOut),
    };
    if outcome.is_err() {
        // 丢弃取消/超时之后仍在通道里的迟到决定，避免被下一次审批误用。
        drain_late_decisions(rx);
    }
    outcome
}

/// 无取消句柄时永不就绪；有则在标志置位时返回。
async fn wait_for_cancel(cancel: Option<&Arc<AtomicBool>>) {
    match cancel {
        None => std::future::pending::<()>().await,
        Some(flag) => {
            while !flag.load(Ordering::SeqCst) {
                tokio::time::sleep(APPROVAL_CANCEL_POLL_INTERVAL).await;
            }
        }
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
        match await_approval_decision(&mut rx_guard, sink.cancel).await {
            Ok(d) => d,
            Err(e) => return Err(e),
        }
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

    #[tokio::test]
    async fn await_approval_decision_returns_decision_when_sent() {
        let (tx, mut rx) = mpsc::channel::<CommandApprovalDecision>(1);
        tx.send(CommandApprovalDecision::AllowOnce).await.unwrap();
        let res = await_approval_decision(&mut rx, None).await;
        assert_eq!(res, Ok(CommandApprovalDecision::AllowOnce));
    }

    #[tokio::test]
    async fn await_approval_decision_returns_cancelled_when_flag_set() {
        // 保持发送端存活：通道空且未关闭，唯一提前出口应为取消分支。
        let (_tx, mut rx) = mpsc::channel::<CommandApprovalDecision>(1);
        let cancel = Arc::new(AtomicBool::new(true));
        let res = await_approval_decision(&mut rx, Some(&cancel)).await;
        assert_eq!(res, Err(ToolApprovalWebError::Cancelled));
    }

    #[tokio::test]
    async fn await_approval_decision_prefers_cancel_over_stale_decision() {
        // 通道内已有「迟到决定」且取消已置位：须判为取消并丢弃残留，避免被放行一次。
        let (tx, mut rx) = mpsc::channel::<CommandApprovalDecision>(1);
        tx.send(CommandApprovalDecision::AllowAlways).await.unwrap();
        let cancel = Arc::new(AtomicBool::new(true));
        let res = await_approval_decision(&mut rx, Some(&cancel)).await;
        assert_eq!(res, Err(ToolApprovalWebError::Cancelled));
        assert!(rx.try_recv().is_err(), "残留决定应已被丢弃");
    }

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
    fn web_timeline_detail_keeps_first_arg_equal_to_command() {
        let spec = ApprovalRequestSpec {
            capability: SensitiveCapability::HostShell,
            sse_command: "curl".to_string(),
            sse_args: "curl -I".to_string(),
            allowlist_key: None,
            cli_title: "t",
            cli_detail: String::new(),
            web_timeline_prefix_zh: "p",
        };
        assert_eq!(web_timeline_detail(&spec), "curl curl -I");
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
