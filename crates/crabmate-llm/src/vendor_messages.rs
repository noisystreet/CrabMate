//! 供应商出站路径：构造 `ChatRequest.messages` 前的 normalize 与 `tool_calls.arguments` 规整。

use std::hash::{Hash, Hasher};

use crabmate_types::Message;

fn system_prompt_stability_info(messages: &[Message]) -> (u64, usize) {
    let sys_content: String = messages
        .iter()
        .filter(|m| m.role == "system")
        .filter_map(|m| m.content.as_ref())
        .filter_map(|c| match c {
            crabmate_types::MessageContent::Text(s) => Some(s.as_str()),
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
        target: "crabmate_llm",
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
    use crabmate_types::sanitize_tool_call_arguments_for_openai_compat;

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

/// 从会话切片构造发往 OpenAI 兼容 API 的 `messages`：**跳过** UI 分隔线与长期记忆注入、按 `preserve_reasoning_on_assistant_tool_calls` 剥离或保留 `reasoning_content`、再 normalize（合并相邻 assistant 等）；`fold_system_into_user` 为真时再 [`crabmate_types::fold_system_messages_into_following_user`]。
#[inline]
pub fn conversation_messages_to_vendor_body(
    messages: &[Message],
    fold_system_into_user: bool,
    preserve_reasoning_on_assistant_tool_calls: bool,
    preserve_deepseek_thinking_reasoning_roundtrip: bool,
) -> Vec<Message> {
    log_system_prompt_stability(messages);
    let mut v = crabmate_types::normalize_messages_for_openai_compatible_request(
        crabmate_types::messages_for_api_stripping_reasoning_skip_ui_separators(
            messages,
            preserve_reasoning_on_assistant_tool_calls,
            preserve_deepseek_thinking_reasoning_roundtrip,
        ),
    );
    if fold_system_into_user {
        v = crabmate_types::fold_system_messages_into_following_user(v);
    }
    sanitize_assistant_tool_call_arguments_for_vendor_in_place(&mut v);
    v
}

/// 与 [`conversation_messages_to_vendor_body`] 相同，但输入已是「已 strip」的 `Vec`（避免重复遍历），仅做 normalize（及可选 system 折叠）。
#[inline]
pub fn normalize_stripped_messages_for_vendor_body(
    messages: Vec<Message>,
    fold_system_into_user: bool,
) -> Vec<Message> {
    let mut v = crabmate_types::normalize_messages_for_openai_compatible_request(messages);
    if fold_system_into_user {
        v = crabmate_types::fold_system_messages_into_following_user(v);
    }
    sanitize_assistant_tool_call_arguments_for_vendor_in_place(&mut v);
    v
}
