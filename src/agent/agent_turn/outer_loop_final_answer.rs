//! L2 单 Agent 外循环：工具已成功但助手终答为空/过短时，注入纠偏并要求自然语言总结。

use crate::types::{Message, last_real_user_message_index, message_content_as_str};

/// 最多注入次数，避免与外层迭代上限死磕。
pub(crate) const OUTER_LOOP_MISSING_FINAL_ANSWER_FEEDBACK_MAX: u32 = 2;

/// 低于该字符数（Unicode scalar）视为「无可见终答」。
pub(crate) const OUTER_LOOP_MISSING_FINAL_ANSWER_MIN_CHARS: usize = 24;

fn outer_loop_window_has_any_successful_tool(messages: &[Message]) -> bool {
    let Some(user_idx) = last_real_user_message_index(messages, false) else {
        return false;
    };
    messages[user_idx.saturating_add(1)..].iter().any(|m| {
        if m.role != "tool" {
            return false;
        }
        let Some(raw) = message_content_as_str(&m.content) else {
            return false;
        };
        if let Some(env) = crate::tool_result::normalize_tool_message_content(raw) {
            return env.ok || env.exit_code == Some(0);
        }
        let lower = raw.to_lowercase();
        lower.contains("退出码：0") || lower.contains("exit code: 0")
    })
}

pub(crate) fn outer_loop_assistant_lacks_visible_final_answer(msg: &Message) -> bool {
    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        return false;
    }
    let text = message_content_as_str(&msg.content).unwrap_or("").trim();
    text.chars().count() < OUTER_LOOP_MISSING_FINAL_ANSWER_MIN_CHARS
}

pub(crate) fn outer_loop_missing_final_answer_feedback_body() -> String {
    format!(
        "{prefix}本轮工具已执行并产生结果，但助手终答为空或过短。请基于**当前对话与工具输出**，用自然语言向用户给出完整终答：\
         说明已完成什么、关键结果/路径/命令输出摘要，以及若仍有未完成项请明确列出。**禁止**再发起无必要的 tool_calls。",
        prefix = crabmate_display_rules::OUTER_LOOP_BUILD_IDLE_ORCHESTRATION_PREFIX
    )
}

/// 若应注入纠偏 user 并继续外循环，返回 `Some(feedback)`。
pub(crate) fn outer_loop_missing_final_answer_feedback_if_needed(
    messages: &[Message],
    assistant: &Message,
    feedback_injected_count: u32,
) -> Option<String> {
    if feedback_injected_count >= OUTER_LOOP_MISSING_FINAL_ANSWER_FEEDBACK_MAX {
        return None;
    }
    if !outer_loop_assistant_lacks_visible_final_answer(assistant) {
        return None;
    }
    if !outer_loop_window_has_any_successful_tool(messages) {
        return None;
    }
    Some(outer_loop_missing_final_answer_feedback_body())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    fn user(text: &str) -> Message {
        Message::user_only(text.to_string())
    }

    fn assistant(text: &str) -> Message {
        Message::assistant_only(text.to_string())
    }

    fn tool_env(name: &str, summary: &str, output: &str) -> Message {
        let parsed = crate::tool_result::parse_legacy_output(name, output);
        Message {
            role: "tool".to_string(),
            content: Some(
                crate::tool_result::encode_tool_message_envelope_v1(
                    name,
                    summary.to_string(),
                    &parsed,
                    output,
                    None,
                )
                .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(name.to_string()),
            tool_call_id: Some("t1".to_string()),
        }
    }

    #[test]
    fn injects_when_tools_succeeded_but_assistant_empty() {
        let msgs = vec![
            user("编译 hello"),
            tool_env(
                "run_command",
                "make",
                "命令：make\n退出码：0\n标准输出：\nBuilt target hello",
            ),
            assistant(""),
        ];
        let fb = outer_loop_missing_final_answer_feedback_if_needed(&msgs, &assistant(""), 0);
        assert!(fb.is_some());
        assert!(fb.unwrap().contains("编排纠偏"));
    }

    #[test]
    fn skips_when_assistant_already_substantive() {
        let msgs = vec![
            user("编译 hello"),
            tool_env("run_command", "make", "命令：make\n退出码：0"),
            assistant("Hello 已编译完成，可执行文件位于 build/hello。"),
        ];
        assert!(
            outer_loop_missing_final_answer_feedback_if_needed(
                &msgs,
                &assistant("Hello 已编译完成，可执行文件位于 build/hello。"),
                0
            )
            .is_none()
        );
    }

    #[test]
    fn skips_without_tool_success() {
        let msgs = vec![user("编译 hello"), assistant("")];
        assert!(
            outer_loop_missing_final_answer_feedback_if_needed(&msgs, &assistant(""), 0).is_none()
        );
    }
}
