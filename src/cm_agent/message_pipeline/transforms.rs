//! 会话侧逐步变换：条数/字符裁剪、tool 压缩、孤立 tool 剔除等（由 [`super::sync_pipeline::apply_session_sync_pipeline_with_config`] 编排）。

use crate::cm_types::{
    Message, MessageContent, is_chat_timeline_marker, is_model_context_artifact_marker,
    message_content_byte_len_for_estimate, user_message_counts_for_branch_truncation,
};

/// 被移出视图前缀的连续区间（以「摘除旁注前的会话视图」坐标为基准）。
///
/// 会话侧管道对消息列表的删除**恒从最旧一端连续进行**，因此被移出的消息总构成
/// 一个连续前缀区间 `[start, start + removed)`。本类型仅记录该**范围**，不持有消息内容：
/// 提交阶段据此从 canonical **逐字切片**取原文，避免捕获到就地压缩后的 tool 正文。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemovedPrefix {
    /// 区间起点（0 基，含）。
    pub start: usize,
    /// 区间长度；`0` 表示未移除任何消息。
    pub removed: usize,
}

impl RemovedPrefix {
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.removed == 0
    }

    #[must_use]
    pub const fn end(self) -> usize {
        self.start.saturating_add(self.removed)
    }

    /// 并入「在当前列表坐标中移除 `[local_start, local_start + local_removed)`」这一段。
    ///
    /// `local_start` 以**已移除本区间后**的列表坐标给出；删除恒从最旧一端连续进行，
    /// 故把局部起点映射回原始坐标后与既有区间取并集，结果仍是连续区间。
    #[must_use]
    pub fn record_local(self, local_start: usize, local_removed: usize) -> Self {
        if local_removed == 0 {
            return self;
        }
        if self.removed == 0 {
            return Self {
                start: local_start,
                removed: local_removed,
            };
        }
        let orig_start = if local_start >= self.start {
            local_start.saturating_add(self.removed)
        } else {
            local_start
        };
        let new_start = self.start.min(orig_start);
        let new_end = self.end().max(orig_start.saturating_add(local_removed));
        Self {
            start: new_start,
            removed: new_end.saturating_sub(new_start),
        }
    }

    /// 与另一区间取并集（两区间均以原始坐标为基准）。
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if other.removed == 0 {
            return self;
        }
        if self.removed == 0 {
            return other;
        }
        let start = self.start.min(other.start);
        let end = self.end().max(other.end());
        Self {
            start,
            removed: end.saturating_sub(start),
        }
    }
}

/// 一个完整用户交互组在会话切片中的半开区间：真实 `user` 到下一条真实 `user` 之前。
///
/// 该区间自然覆盖其后的 `assistant(tool_calls)`、一个或多个 `tool` 结果以及最终 assistant，
/// 以及服务端命名注入的 `user`，因而按组删除不会制造孤立工具结果。首条 `system` 与首个真实
/// `user` 之前的前缀不属于任何组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConversationTurnGroup {
    pub start: usize,
    pub end: usize,
}

/// 扫描会话中的完整用户交互组。末组可包含当前正在进行的工具链，调用方通常应始终保留。
#[must_use]
pub fn conversation_turn_groups(messages: &[Message]) -> Vec<ConversationTurnGroup> {
    let starts: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(idx, message)| {
            user_message_counts_for_branch_truncation(message).then_some(idx)
        })
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(idx, start)| ConversationTurnGroup {
            start: *start,
            end: starts.get(idx + 1).copied().unwrap_or(messages.len()),
        })
        .collect()
}

/// 删除最旧完整交互组，同时至少保留 `min_groups_to_keep` 个最近组。
///
/// 返回被移除的**连续前缀区间**；没有可删除完整组时返回空区间。
pub fn remove_oldest_turn_group(
    messages: &mut Vec<Message>,
    min_groups_to_keep: usize,
) -> RemovedPrefix {
    let groups = conversation_turn_groups(messages);
    if groups.len() <= min_groups_to_keep {
        return RemovedPrefix::default();
    }
    let oldest = groups[0];
    let removed = RemovedPrefix {
        start: oldest.start,
        removed: oldest.end.saturating_sub(oldest.start),
    };
    messages.drain(oldest.start..oldest.end);
    removed
}

/// 抽出 UI 时间线与模型视图工件旁注，避免占用 `max_message_history` / 字符预算。
pub fn take_chat_timeline_markers(messages: &mut Vec<Message>) -> Vec<Message> {
    let mut parked = Vec::new();
    let mut kept = Vec::with_capacity(messages.len());
    for m in messages.drain(..) {
        if is_chat_timeline_marker(&m) || is_model_context_artifact_marker(&m) {
            parked.push(m);
        } else {
            kept.push(m);
        }
    }
    *messages = kept;
    parked
}

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
        if let crate::cm_types::MessageContent::Text(s) = c
            && let Some(compressed) =
                crate::cm_tools::tool_result::maybe_compress_tool_message_content(s, max_chars)
        {
            *s = compressed;
            n += 1;
        }
    }
    n
}

fn trim_tail_after_system(after: &[Message], tail_keep: usize) -> Vec<Message> {
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
    tail
}

/// 保留首条 `system`，其后最多保留 `max_after_system` 条消息（与历史 `max_message_history` 语义一致）。
///
/// 与 `runtime/workspace_session` 加载截断一致：若保留的尾部以**两条连续** `assistant` 开头，且被裁掉的前缀里仍有 `user`，则插回其中最后一条 `user`（并丢掉一条较旧消息以维持条数上限），避免 `[system, assistant, assistant, …]` 触发 400。
/// 返回被移除的**连续前缀区间**（按完整交互组删除的部分）；病理兜底分支不构成纯前缀删除，返回空区间。
/// 「是否发生裁剪」请由调用方比较调用前后的条数判定。
pub fn trim_messages_by_count(
    messages: &mut Vec<Message>,
    max_after_system: usize,
) -> RemovedPrefix {
    if messages.is_empty() || max_after_system == 0 {
        return RemovedPrefix::default();
    }
    let has_system_head = messages[0].role == "system";
    let max_total = max_after_system.saturating_add(usize::from(has_system_head));
    let mut removed = RemovedPrefix::default();
    while messages.len() > max_total {
        let step = remove_oldest_turn_group(messages, 1);
        if step.is_empty() {
            break;
        }
        removed = removed.union(step);
    }
    if messages.len() <= max_total {
        return removed;
    }
    if !conversation_turn_groups(messages).is_empty() {
        // 最近完整组本身可超过条数兜底；结构完整性优先于硬切消息条数。
        return removed;
    }

    // 无完整 user 组的病理历史沿用旧尾部兜底；正常 ReAct 历史不会进入此分支。
    if messages[0].role == "system" {
        if messages.len() <= 1 + max_after_system {
            return removed;
        }
        let sys = messages[0].clone();
        let tail = trim_tail_after_system(&messages[1..], max_after_system);
        let mut out = vec![sys];
        out.extend(tail);
        *messages = out;
    } else if messages.len() > max_after_system {
        let skip = messages.len() - max_after_system;
        *messages = messages.iter().skip(skip).cloned().collect();
    }
    removed
}

fn trim_complete_turn_groups_by_char_budget(
    messages: &mut Vec<Message>,
    budget: usize,
    min_total: usize,
) -> RemovedPrefix {
    let mut removed = RemovedPrefix::default();
    while estimate_non_system_chars(messages) > budget {
        let groups = conversation_turn_groups(messages);
        let Some(oldest) = groups.first().copied() else {
            break;
        };
        let remaining = messages
            .len()
            .saturating_sub(oldest.end.saturating_sub(oldest.start));
        if groups.len() <= 1 || remaining < min_total {
            break;
        }
        messages.drain(oldest.start..oldest.end);
        removed = removed.record_local(
            oldest.start,
            oldest.end.saturating_sub(oldest.start),
        );
    }
    removed
}

fn trim_ungrouped_messages_by_char_budget(
    messages: &mut Vec<Message>,
    budget: usize,
    min_total: usize,
) -> RemovedPrefix {
    let start_idx = usize::from(messages[0].role == "system");
    let removable = messages.len().saturating_sub(min_total);
    let mut remaining_chars = estimate_non_system_chars(messages);
    let mut remove_count = 0usize;
    for msg in messages.iter().skip(start_idx).take(removable) {
        if remaining_chars <= budget {
            break;
        }
        remaining_chars = remaining_chars.saturating_sub(estimate_message_chars(msg));
        remove_count += 1;
    }
    if remove_count > 0 {
        messages.drain(start_idx..start_idx + remove_count);
    }
    RemovedPrefix {
        start: start_idx,
        removed: remove_count,
    }
}

/// 在已压缩 tool 的前提下删除最旧完整交互组，直到非 system 字符 ≤ `budget` 或条数触底。
/// 无 user 组的病理历史保留逐消息兼容兜底。返回被移除的**连续前缀区间**。
pub fn trim_messages_by_char_budget(
    messages: &mut Vec<Message>,
    budget: usize,
    min_messages_after_system: usize,
) -> RemovedPrefix {
    if budget == 0 || messages.len() <= 1 {
        return RemovedPrefix::default();
    }
    let min_total = 1 + min_messages_after_system;
    if messages.len() <= min_total {
        return RemovedPrefix::default();
    }
    if estimate_non_system_chars(messages) <= budget {
        return RemovedPrefix::default();
    }
    if conversation_turn_groups(messages).is_empty() {
        // 无 user 组时保留旧的逐消息兜底，避免损坏历史兼容性。
        trim_ungrouped_messages_by_char_budget(messages, budget, min_total)
    } else {
        trim_complete_turn_groups_by_char_budget(messages, budget, min_total)
    }
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

/// 中断回合补齐的占位 `tool` 正文（纯文本，非工具信封）：说明该工具因用户中断未执行完成。
const INTERRUPTED_TOOL_RESULT_PLACEHOLDER: &str =
    "本轮已被用户中断，该工具调用未执行完成，无结果。";

/// 补齐**最后一条**带 `tool_calls` 的 `assistant` 里尚未得到响应的 `tool` 消息，返回新增条数。
///
/// 用户在工具批执行中途点「停止生成」时，落盘的 `messages` 可能停在
/// `assistant(tool_calls=[A, B]) + tool(A)` 这种**部分响应**状态：末条是 `tool`（不是 assistant），
/// [`crate::cm_types::normalize_messages_for_openai_compatible_request`] 只清理「末条 assistant 的悬空 `tool_calls`」，
/// 覆盖不到该形态 ⇒ 下一轮请求会因「assistant 的 `tool_calls` 缺少对应 tool 响应」被上游 400。
/// 这里为缺失的 `tool_call_id` 合成占位 `tool` 消息（`content` 为纯文本说明），使序列合法。
///
/// **仅用于中断 / 取消落盘路径**（写回 canonical 前调用），不改动正常完成回合的既有行为。
pub fn complete_pending_tool_responses_for_persistence(messages: &mut Vec<Message>) -> usize {
    let Some(assistant_idx) = messages.iter().rposition(|message| {
        message.role.trim().eq_ignore_ascii_case("assistant")
            && message
                .tool_calls
                .as_ref()
                .is_some_and(|calls| !calls.is_empty())
    }) else {
        return 0;
    };
    let tool_calls = messages[assistant_idx]
        .tool_calls
        .clone()
        .unwrap_or_default();
    let mut insert_at = assistant_idx + 1;
    let mut responded: Vec<String> = Vec::new();
    // 角色判定与 assistant 分支同口径（trim + 忽略大小写），避免上游写入 `"Tool"` / 带空白时漏判。
    // 已响应的 `tool` 消息若 `tool_call_id == None`，继续推进 `insert_at` 而不 panic：该消息无法
    // 对应到任何 pending call，只能靠后续按 id 补齐，直接丢弃它会破坏既有序列。
    while insert_at < messages.len()
        && messages[insert_at].role.trim().eq_ignore_ascii_case("tool")
    {
        if let Some(id) = messages[insert_at].tool_call_id.clone() {
            responded.push(id);
        }
        insert_at += 1;
    }
    let placeholders: Vec<Message> = tool_calls
        .into_iter()
        .filter(|call| !responded.contains(&call.id))
        .map(|call| Message {
            role: "tool".to_string(),
            content: Some(MessageContent::Text(format!(
                "{INTERRUPTED_TOOL_RESULT_PLACEHOLDER}（工具：{}）",
                call.function.name
            ))),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some(call.id),
        })
        .collect();
    let added = placeholders.len();
    if added > 0 {
        messages.splice(insert_at..insert_at, placeholders);
    }
    added
}
