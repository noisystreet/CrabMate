//! 多轮续接辅助：工具失败后续跑短句识别、近期 tool 失败探测。
//!
//! 有效用户任务抽取见 `agent_turn::intent::user`（确认流 / 失败续跑）。

use crabmate_types::Message;

/// 用户在上轮工具失败后发送的短续跑句（不含单独「继续执行」类确认词，见 [`crate::intent_router::is_explicit_execute_confirmation`]）。
pub fn is_resume_after_failure_utterance(s: &str) -> bool {
    let t = s.trim().to_lowercase();
    matches!(
        t.as_str(),
        "继续" | "接着" | "再来" | "重试" | "接着来" | "continue" | "retry" | "go on"
    )
}

/// 在 `messages` 尾部窗口内是否存在失败态的 `role: tool`（不读参数正文）。
pub fn messages_have_recent_tool_failure(messages: &[Message], max_tail: usize) -> bool {
    for m in messages.iter().rev().take(max_tail) {
        if m.role != "tool" {
            continue;
        }
        let Some(t) = crabmate_types::message_content_as_str(&m.content) else {
            continue;
        };
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t.trim())
            && let Some(env) = v.get("crabmate_tool")
            && env.get("ok").and_then(|x| x.as_bool()) == Some(false)
        {
            return true;
        }
        // 无信封时的保守启发式（不依赖具体供应商）
        let low = t.to_lowercase();
        if low.contains("\"ok\":false")
            || (low.contains("ok") && (low.contains("error_code") || low.contains("失败")))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_types::Message;

    #[test]
    fn resume_utterances() {
        assert!(is_resume_after_failure_utterance("继续"));
        assert!(is_resume_after_failure_utterance("retry"));
        assert!(!is_resume_after_failure_utterance("继续执行这个大任务"));
    }

    #[test]
    fn detects_recent_tool_failure_envelope() {
        let messages = vec![
            Message::user_only("x".to_string()),
            Message {
                role: "tool".into(),
                content: Some(crabmate_types::MessageContent::Text(
                    r#"{"crabmate_tool":{"ok":false,"summary":"boom"}}"#.into(),
                )),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".into()),
            },
        ];
        assert!(messages_have_recent_tool_failure(&messages, 8));
    }
}
