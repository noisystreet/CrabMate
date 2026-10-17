//! 为缺失 `tool` 响应的 `assistant.tool_calls` 补齐占位消息（出站拼装与取消落盘共用）。

use super::{Message, MessageContent, assistant_has_non_empty_tool_calls, is_assistant_role};

#[inline]
fn is_tool_role(role: &str) -> bool {
    role.trim().eq_ignore_ascii_case("tool")
}

/// 紧邻 `start` 起的连续 `tool` 消息的 `tool_call_id` 与结束下标。
fn contiguous_tool_response_ids(messages: &[Message], start: usize) -> (Vec<String>, usize) {
    let mut ids = Vec::new();
    let mut idx = start;
    while idx < messages.len() && is_tool_role(&messages[idx].role) {
        if let Some(id) = messages[idx].tool_call_id.as_ref() {
            ids.push(id.clone());
        }
        idx += 1;
    }
    (ids, idx)
}

fn tool_placeholder_message(tool_call_id: &str, tool_name: &str, prefix: &str) -> Message {
    Message {
        role: "tool".to_string(),
        content: Some(MessageContent::Text(format!(
            "{prefix}（工具：{tool_name}）"
        ))),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: Some(tool_call_id.to_string()),
    }
}

/// 为**所有**「`assistant.tool_calls` 缺少紧邻 `tool` 响应」的调用补齐占位 `tool` 消息，返回新增条数。
///
/// 上游（OpenAI / DeepSeek 等）要求 `assistant(tool_calls=[…])` 后必须紧跟每个 `tool_call_id`
/// 的 `role: "tool"` 消息。以下情形会产生非法序列：
/// - 流式输出中途被取消，`tool_calls` 只累积了一半且从未执行；
/// - 历史裁剪删掉了 `tool` 消息，却保留了带 `tool_calls` 的 `assistant`；
/// - 会话导入 / 旧版本落盘的脏数据。
///
/// 占位正文只是纯文本说明（非工具信封），用于满足结构校验并让模型知道该调用没有结果。
/// 出站拼装与取消落盘两条路径共用本函数；`placeholder_prefix` 区分场景措辞。
pub fn complete_missing_tool_responses(
    messages: &mut Vec<Message>,
    placeholder_prefix: &str,
) -> usize {
    let mut added = 0usize;
    let mut idx = 0usize;
    while idx < messages.len() {
        if !is_assistant_role(&messages[idx].role)
            || !assistant_has_non_empty_tool_calls(&messages[idx])
        {
            idx += 1;
            continue;
        }
        let calls = messages[idx].tool_calls.clone().unwrap_or_default();
        let (responded, insert_at) = contiguous_tool_response_ids(messages, idx + 1);
        let placeholders: Vec<Message> = calls
            .iter()
            .filter(|call| !responded.iter().any(|id| id == &call.id))
            .map(|call| tool_placeholder_message(&call.id, &call.function.name, placeholder_prefix))
            .collect();
        let n = placeholders.len();
        if n > 0 {
            messages.splice(insert_at..insert_at, placeholders);
            added += n;
        }
        idx = insert_at + n;
    }
    added
}
