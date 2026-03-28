//! Web SSE：审批决策后的时间线旁注（`timeline_log`），与 `tool_registry` / 工作流共用。

use tokio::sync::mpsc;

use crate::types::CommandApprovalDecision;

use super::{SsePayload, TimelineLogBody, encode_message, send_string_logged};

/// 与 UI 时间线标题一致的中文简述。
pub(crate) fn command_approval_decision_label_zh(d: CommandApprovalDecision) -> &'static str {
    match d {
        CommandApprovalDecision::Deny => "拒绝",
        CommandApprovalDecision::AllowOnce => "本次允许",
        CommandApprovalDecision::AllowAlways => "永久允许",
    }
}

pub(crate) async fn send_timeline_approval_decision(
    out_tx: &mpsc::Sender<String>,
    title_prefix: &str,
    detail: Option<String>,
    decision: CommandApprovalDecision,
    log_tag: &'static str,
) {
    let zh = command_approval_decision_label_zh(decision);
    let line = encode_message(SsePayload::TimelineLog {
        log: TimelineLogBody {
            kind: "approval_decision".to_string(),
            title: format!("{title_prefix}{zh}"),
            detail,
        },
    });
    let _ = send_string_logged(out_tx, line, log_tag).await;
}
