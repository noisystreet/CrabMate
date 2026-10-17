//! 供应商出站路径：构造 `ChatRequest.messages` 前的 normalize 与 `tool_calls.arguments` 规整。

use std::hash::{Hash, Hasher};

use crate::cm_types::Message;

fn system_prompt_stability_info(messages: &[Message]) -> (u64, usize) {
    let sys_content: String = messages
        .iter()
        .filter(|m| m.role == "system")
        .filter_map(|m| m.content.as_ref())
        .filter_map(|c| match c {
            crate::cm_types::MessageContent::Text(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    let len = sys_content.len();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    sys_content.hash(&mut hasher);
    (hasher.finish(), len)
}

/// 记录 system prompt 稳定性信息（hash + 字符数），辅助判断前缀缓存稳定性。
pub fn log_system_prompt_stability(messages: &[Message]) {
    let (hash, len) = system_prompt_stability_info(messages);
    log::info!(
        target: "crate::cm_llm",
        "system_prompt_stability hash={:x} chars={}",
        hash,
        len
    );
}

fn single_line_preview(s: &str, max_chars: usize) -> String {
    let folded = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if max_chars == 0 {
        return String::new();
    }
    let mut iter = folded.chars();
    let prefix: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{prefix}…(truncated)")
    } else {
        prefix
    }
}

fn sanitize_assistant_tool_call_arguments_for_vendor_in_place(msgs: &mut [Message]) {
    use crate::cm_types::sanitize_tool_call_arguments_for_openai_compat;

    for m in msgs.iter_mut() {
        if !m.role.trim().eq_ignore_ascii_case("assistant") {
            continue;
        }
        let Some(tcs) = m.tool_calls.as_mut() else {
            continue;
        };
        for tc in tcs.iter_mut() {
            let orig = tc.function.arguments.as_str();
            let s = sanitize_tool_call_arguments_for_openai_compat(orig);
            if s == tc.function.arguments {
                continue;
            }
            let trimmed_empty = orig.trim().is_empty();
            if trimmed_empty {
                log::debug!(
                    target: "crabmate",
                    "tool_calls.function.arguments 空串已规范为 {{}} tool_call_id={}",
                    tc.id
                );
            } else if s == "{}" && serde_json::from_str::<serde_json::Value>(orig.trim()).is_err() {
                log::warn!(
                    target: "crabmate",
                    "tool_calls.function.arguments 无法解析为 JSON（已替换为 {{}} 以满足上游校验；常见原因：流式截断、字符串内未转义换行、模型输出非 JSON）。tool_call_id={} preview={}",
                    tc.id,
                    single_line_preview(orig, 120)
                );
            } else {
                log::debug!(
                    target: "crabmate",
                    "tool_calls.function.arguments 已规整为合法 JSON 形态 tool_call_id={}",
                    tc.id
                );
            }
            tc.function.arguments = s;
        }
    }
}

/// 出站补齐的占位 `tool` 正文：历史里该调用没有结果（多为取消 / 裁剪 / 旧版本落盘）。
const VENDOR_MISSING_TOOL_RESULT_PLACEHOLDER: &str =
    "该工具调用在会话历史中没有结果（可能因生成被中断或历史被裁剪），未执行完成，无结果。";

/// 出站前补齐缺失的 tool 响应，并记录条数。
///
/// 存量脏会话（`assistant.tool_calls` 后没有对应 `tool` 消息）若不在此自愈，每一次后续请求
/// 都会被上游以 HTTP 400 拒绝，表现为「停止后再发消息永远拿不到回复」。
fn complete_missing_tool_responses_for_vendor_body(v: &mut Vec<Message>) {
    let added =
        crate::cm_types::complete_missing_tool_responses(v, VENDOR_MISSING_TOOL_RESULT_PLACEHOLDER);
    if added > 0 {
        log::warn!(
            target: "crabmate",
            "出站消息补齐 {} 条缺失的 tool 响应（会话历史中 assistant.tool_calls 无对应 tool 消息）",
            added
        );
    }
}

/// 从会话切片构造发往 OpenAI 兼容 API 的 `messages`：**跳过** UI 分隔线与长期记忆注入、按 `preserve_reasoning_on_assistant_tool_calls` 剥离或保留 `reasoning_content`、再 normalize（合并相邻 assistant 等）；`fold_system_into_user` 为真时再 [`crate::cm_types::fold_system_messages_into_following_user`]。
#[inline]
pub fn conversation_messages_to_vendor_body(
    messages: &[Message],
    fold_system_into_user: bool,
    preserve_reasoning_on_assistant_tool_calls: bool,
    preserve_deepseek_thinking_reasoning_roundtrip: bool,
) -> Vec<Message> {
    log_system_prompt_stability(messages);
    let mut stripped = crate::cm_types::messages_for_api_stripping_reasoning_skip_ui_separators(
        messages,
        preserve_reasoning_on_assistant_tool_calls,
        preserve_deepseek_thinking_reasoning_roundtrip,
    );
    // 在 strip 之后、normalize / fold 之前补齐：此时 assistant 与其 tool 响应仍紧邻，
    // 后续合并相邻 assistant 也不会跨过新插入的 tool 消息。
    complete_missing_tool_responses_for_vendor_body(&mut stripped);
    let mut v = crate::cm_types::normalize_messages_for_openai_compatible_request(stripped);
    if fold_system_into_user {
        v = crate::cm_types::fold_system_messages_into_following_user(v);
    }
    sanitize_assistant_tool_call_arguments_for_vendor_in_place(&mut v);
    v
}

/// 与 [`conversation_messages_to_vendor_body`] 相同，但输入已是「已 strip」的 `Vec`（避免重复遍历），仅做 normalize（及可选 system 折叠）。
#[inline]
pub fn normalize_stripped_messages_for_vendor_body(
    mut messages: Vec<Message>,
    fold_system_into_user: bool,
) -> Vec<Message> {
    complete_missing_tool_responses_for_vendor_body(&mut messages);
    let mut v = crate::cm_types::normalize_messages_for_openai_compatible_request(messages);
    if fold_system_into_user {
        v = crate::cm_types::fold_system_messages_into_following_user(v);
    }
    sanitize_assistant_tool_call_arguments_for_vendor_in_place(&mut v);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::{FunctionCall, ToolCall};

    fn assistant_with_call(args: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(crate::cm_types::MessageContent::Text("calling".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_string(),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: "run_command".to_string(),
                    arguments: args.to_string(),
                },
            }]),
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn dirty_history_gets_missing_tool_response_before_send() {
        let messages = vec![
            Message::user_only("u"),
            assistant_with_call("{\"command\":\"bash\""),
            Message::user_only("continue"),
        ];

        let v = conversation_messages_to_vendor_body(&messages, false, false, false);

        let assistant_idx = v
            .iter()
            .position(|m| m.role == "assistant")
            .expect("assistant");
        assert_eq!(v[assistant_idx + 1].role, "tool");
        assert_eq!(v[assistant_idx + 1].tool_call_id.as_deref(), Some("call_1"));
    }
}
