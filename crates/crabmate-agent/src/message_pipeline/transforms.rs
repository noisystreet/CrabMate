//! 会话侧逐步变换：条数/字符裁剪、tool 压缩、孤立 tool 剔除等（由 [`super::sync_pipeline::apply_session_sync_pipeline_with_config`] 编排）。

use crabmate_types::{Message, message_content_byte_len_for_estimate};

/// 从字节长度近似字符数：ASCII 约 1:1，CJK 约 3:1，混合取中间值 ~2:1。
fn estimate_chars_from_bytes(s: &str) -> usize {
    s.len().div_ceil(2)
}

/// 估算单条消息占用的「约等于字符数」（用于预算；非精确 token）。
/// 使用字节长度近似，避免对大内容做 O(n) 的 `chars().count()`。
pub fn estimate_message_chars(m: &Message) -> usize {
    let mut n = message_content_byte_len_for_estimate(&m.content).div_ceil(2);
    n = n.saturating_add(
        m.reasoning_content
            .as_deref()
            .map(estimate_chars_from_bytes)
            .unwrap_or(0),
    );
    if let Some(ref tcs) = m.tool_calls {
        for tc in tcs {
            n = n.saturating_add(tc.function.name.len());
            n = n.saturating_add(tc.function.arguments.len());
            n = n.saturating_add(tc.id.len());
        }
    }
    n
}

/// 除 `system` 外所有消息的近似字符总和。
pub fn estimate_non_system_chars(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| m.role != "system")
        .map(estimate_message_chars)
        .sum()
}

/// 截断 `tool` 消息正文（过长时追加说明尾注）；返回本轮压缩的 `tool` 条数。
pub fn compress_tool_message_contents(messages: &mut [Message], max_chars: usize) -> usize {
    let max_chars = max_chars.max(256);
    let mut n = 0usize;
    for m in messages.iter_mut() {
        if m.role != "tool" {
            continue;
        }
        let Some(c) = &mut m.content else {
            continue;
        };
        if let crabmate_types::MessageContent::Text(s) = c
            && let Some(compressed) =
                crabmate_internal::tool_result::maybe_compress_tool_message_content(s, max_chars)
        {
            *s = compressed;
            n += 1;
        }
    }
    n
}

/// 保留首条 `system`，其后最多保留 `max_after_system` 条消息（与历史 `max_message_history` 语义一致）。
///
/// 与 `runtime/workspace_session` 加载截断一致：若保留的尾部以**两条连续** `assistant` 开头，且被裁掉的前缀里仍有 `user`，则插回其中最后一条 `user`（并丢掉一条较旧消息以维持条数上限），避免 `[system, assistant, assistant, …]` 触发 400。
/// 返回是否**删除了**至少一条消息（条数裁剪生效）。
pub fn trim_messages_by_count(messages: &mut Vec<Message>, max_after_system: usize) -> bool {
    if messages.is_empty() || max_after_system == 0 {
        return false;
    }
    let before = messages.len();
    if messages[0].role == "system" {
        if messages.len() <= 1 + max_after_system {
            return false;
        }
        let sys = messages[0].clone();
        let after: Vec<Message> = messages[1..].to_vec();
        let tail_keep = max_after_system;
        let skip = after.len().saturating_sub(tail_keep);
        let mut tail: Vec<Message> = after.iter().skip(skip).cloned().collect();
        let tail_opens_with_assistant_run = tail.len() >= 2
            && tail[0].role.trim().eq_ignore_ascii_case("assistant")
            && tail[1].role.trim().eq_ignore_ascii_case("assistant");
        if tail_opens_with_assistant_run
            && let Some(ui) = after[..skip]
                .iter()
                .rposition(|m| m.role.trim().eq_ignore_ascii_case("user"))
        {
            tail.insert(0, after[ui].clone());
            while tail.len() > tail_keep {
                if tail.len() <= 1 {
                    break;
                }
                tail.remove(1);
            }
        }
        let mut out = vec![sys];
        out.extend(tail);
        *messages = out;
    } else if messages.len() > max_after_system {
        let skip = messages.len() - max_after_system;
        *messages = messages.iter().skip(skip).cloned().collect();
    }
    messages.len() < before
}

/// 在已压缩 tool 的前提下，从索引 1 起删除最旧消息，直到非 system 字符 ≤ `budget` 或条数触底。
/// 返回是否**删除了**至少一条消息（字符预算裁剪生效）。
pub fn trim_messages_by_char_budget(
    messages: &mut Vec<Message>,
    budget: usize,
    min_messages_after_system: usize,
) -> bool {
    if budget == 0 || messages.len() <= 1 {
        return false;
    }
    let min_total = 1 + min_messages_after_system;
    if messages.len() <= min_total {
        return false;
    }
    let current_chars = estimate_non_system_chars(messages);
    if current_chars <= budget {
        return false;
    }

    let has_system_head = messages[0].role == "system";
    let start_idx = if has_system_head { 1 } else { 0 };
    let removable = messages.len().saturating_sub(min_total);
    if removable == 0 {
        return false;
    }

    let mut remaining_chars = current_chars;
    let mut remove_count = 0usize;
    for msg in messages.iter().skip(start_idx).take(removable) {
        if remaining_chars <= budget {
            break;
        }
        remaining_chars = remaining_chars.saturating_sub(estimate_message_chars(msg));
        remove_count += 1;
    }
    if remove_count == 0 {
        return false;
    }
    messages.drain(start_idx..start_idx + remove_count);
    true
}

/// 删除「无前驱 `assistant` + `tool_calls`」的 `role: tool` 消息。
///
/// 按条数/字符裁剪历史时，可能截掉带 `tool_calls` 的 `assistant`，却保留其后的 `tool`，
/// OpenAI 兼容 API 会返回 400：`Messages with role 'tool' must be a response to a preceding message with 'tool_calls'`。
/// 返回被删除的 `role: tool` 条数。
pub fn drop_orphan_tool_messages(messages: &mut Vec<Message>) -> usize {
    let before_len = messages.len();
    let mut keep = vec![true; messages.len()];
    for i in 0..messages.len() {
        if messages[i].role != "tool" {
            continue;
        }
        let has_valid_predecessor = i > 0 && {
            let mut prev = i - 1;
            while prev > 0 && !keep[prev] {
                prev -= 1;
            }
            keep[prev]
                && (messages[prev].role == "tool"
                    || (messages[prev].role == "assistant"
                        && messages[prev]
                            .tool_calls
                            .as_ref()
                            .is_some_and(|c| !c.is_empty())))
        };
        if !has_valid_predecessor {
            keep[i] = false;
        }
    }
    let mut idx = 0;
    messages.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });
    before_len.saturating_sub(messages.len())
}
