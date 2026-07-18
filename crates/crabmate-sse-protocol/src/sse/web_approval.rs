//! Web SSE：审批决策后的时间线旁注（`timeline_log`），与 `tool_registry` / 工作流共用。

use tokio::sync::mpsc;

use crabmate_types::CommandApprovalDecision;

use super::{SsePayload, TimelineLogBody, encode_message, send_string_logged};

/// 与 UI 时间线标题一致的中文简述。
pub fn command_approval_decision_label_zh(d: CommandApprovalDecision) -> &'static str {
    match d {
        CommandApprovalDecision::Deny => "拒绝",
        CommandApprovalDecision::AllowOnce => "本次允许",
        CommandApprovalDecision::AllowAlways => "永久允许",
    }
}

pub async fn send_timeline_approval_decision(
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

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use crabmate_types::CommandApprovalDecision;

    use super::*;

    #[test]
    fn command_approval_decision_labels_zh() {
        assert_eq!(
            command_approval_decision_label_zh(CommandApprovalDecision::Deny),
            "拒绝"
        );
        assert_eq!(
            command_approval_decision_label_zh(CommandApprovalDecision::AllowOnce),
            "本次允许"
        );
        assert_eq!(
            command_approval_decision_label_zh(CommandApprovalDecision::AllowAlways),
            "永久允许"
        );
    }

    #[tokio::test]
    async fn send_timeline_approval_decision_encodes_timeline_log() {
        let (tx, mut rx) = mpsc::channel::<String>(2);
        send_timeline_approval_decision(
            &tx,
            "审批：",
            Some("git status".to_string()),
            CommandApprovalDecision::AllowOnce,
            "test_tag",
        )
        .await;
        drop(tx);
        let line = rx.recv().await.expect("line");
        assert!(line.contains("approval_decision"));
        assert!(line.contains("本次允许"));
        assert!(line.contains("git status"));
    }
}
