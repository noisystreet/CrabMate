//! 对话 **`Message` 变换管道**：统一「会话内存侧」与「发往供应商」两阶段的处理顺序与可观测性。
//!
//! ## 两阶段
//!
//! 1. **会话同步（`apply_session_sync_pipeline`）**：在每次调用模型前对**进程内** `Vec<Message>` 就地处理——工具正文压缩、条数/字符裁剪、孤立 `tool` 剔除、合并相邻 `assistant`（保留会话尾部空占位语义，见 [`crate::types::normalize_messages_for_openai_compatible_request`] 文档）。实现原在 [`super::context_window`]，现经本模块编排。
//! 2. **供应商出站（`conversation_messages_to_vendor_body` 等）**：从会话切片构造 **`ChatRequest.messages`**：跳过 UI 分隔线与长期记忆注入、去掉 `reasoning_content`、再经 OpenAI 兼容 normalize（合并相邻 assistant、清理尾部非法 assistant）。**不**写入会话 `Vec`。
//!
//! 新增处理步骤时：优先在本文件增加 `MessagePipelineStage` 变体，并在 `apply_session_sync_pipeline` 中按固定顺序调用，避免在 `agent_turn` / `llm` 多处散落。

use crate::config::AgentConfig;
use crate::types::Message;

// ── 会话侧：估算与逐步变换（由 `apply_session_sync_pipeline` 编排）────────────

/// 从字节长度近似字符数：ASCII 约 1:1，CJK 约 3:1，混合取中间值 ~2:1。
fn estimate_chars_from_bytes(s: &str) -> usize {
    s.len().div_ceil(2)
}

/// 估算单条消息占用的「约等于字符数」（用于预算；非精确 token）。
/// 使用字节长度近似，避免对大内容做 O(n) 的 `chars().count()`。
pub fn estimate_message_chars(m: &Message) -> usize {
    let mut n = m
        .content
        .as_deref()
        .map(estimate_chars_from_bytes)
        .unwrap_or(0);
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

/// 截断 `tool` 消息正文（过长时追加说明尾注）。
pub fn compress_tool_message_contents(messages: &mut [Message], max_chars: usize) {
    let max_chars = max_chars.max(256);
    for m in messages.iter_mut() {
        if m.role != "tool" {
            continue;
        }
        let Some(ref c) = m.content else {
            continue;
        };
        if let Some(compressed) =
            crate::tool_result::maybe_compress_tool_message_content(c, max_chars)
        {
            m.content = Some(compressed);
        }
    }
}

/// 保留首条 `system`，其后最多保留 `max_after_system` 条消息（与历史 `max_message_history` 语义一致）。
///
/// 与 `runtime/workspace_session` 加载截断一致：若保留的尾部以**两条连续** `assistant` 开头，且被裁掉的前缀里仍有 `user`，则插回其中最后一条 `user`（并丢掉一条较旧消息以维持条数上限），避免 `[system, assistant, assistant, …]` 触发 400。
pub fn trim_messages_by_count(messages: &mut Vec<Message>, max_after_system: usize) {
    if messages.is_empty() || max_after_system == 0 {
        return;
    }
    if messages[0].role == "system" {
        if messages.len() <= 1 + max_after_system {
            return;
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
}

/// 在已压缩 tool 的前提下，从索引 1 起删除最旧消息，直到非 system 字符 ≤ `budget` 或条数触底。
pub fn trim_messages_by_char_budget(
    messages: &mut Vec<Message>,
    budget: usize,
    min_messages_after_system: usize,
) {
    if budget == 0 || messages.len() <= 1 {
        return;
    }
    let min_total = 1 + min_messages_after_system;
    if messages.len() <= min_total {
        return;
    }
    let current_chars = estimate_non_system_chars(messages);
    if current_chars <= budget {
        return;
    }

    let has_system_head = messages[0].role == "system";
    let start_idx = if has_system_head { 1 } else { 0 };
    let removable = messages.len().saturating_sub(min_total);
    if removable == 0 {
        return;
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
        return;
    }
    messages.drain(start_idx..start_idx + remove_count);
}

/// 删除「无前驱 `assistant` + `tool_calls`」的 `role: tool` 消息。
///
/// 按条数/字符裁剪历史时，可能截掉带 `tool_calls` 的 `assistant`，却保留其后的 `tool`，
/// OpenAI 兼容 API 会返回 400：`Messages with role 'tool' must be a response to a preceding message with 'tool_calls'`。
pub fn drop_orphan_tool_messages(messages: &mut Vec<Message>) {
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
}

// ── 管道编排与可观测性 ───────────────────────────────────────────────────────

/// 会话同步管道中「一步」的标签（顺序即 [`apply_session_sync_pipeline`] 中的执行顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessagePipelineStage {
    /// 管道入口（尚未改动 `messages`）。
    SessionSyncStart,
    AfterCompressTool,
    AfterTrimByCount,
    AfterTrimByCharBudget,
    AfterSecondCompressTool,
    AfterDropOrphanTool,
    AfterMergeAssistantsInPlace,
}

impl MessagePipelineStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::SessionSyncStart => "start",
            Self::AfterCompressTool => "after_compress_tool",
            Self::AfterTrimByCount => "after_trim_count",
            Self::AfterTrimByCharBudget => "after_trim_char_budget",
            Self::AfterSecondCompressTool => "after_compress_tool_2",
            Self::AfterDropOrphanTool => "after_drop_orphan_tool",
            Self::AfterMergeAssistantsInPlace => "after_merge_assistants",
        }
    }
}

/// 一步后的消息列表度量（供 debug 日志）。
#[derive(Clone, Debug)]
pub struct PipelineStepSnapshot {
    pub stage: MessagePipelineStage,
    pub message_count: usize,
    pub non_system_chars_est: usize,
}

/// 会话同步管道的一次执行轨迹（仅 debug 启用时填充）。
#[derive(Default, Debug)]
pub struct MessagePipelineReport {
    pub steps: Vec<PipelineStepSnapshot>,
}

impl MessagePipelineReport {
    fn record(&mut self, stage: MessagePipelineStage, messages: &[Message]) {
        self.steps.push(PipelineStepSnapshot {
            stage,
            message_count: messages.len(),
            non_system_chars_est: estimate_non_system_chars(messages),
        });
    }

    /// 单行摘要，便于 `tracing` / `log`。
    pub fn format_for_log(&self) -> String {
        let parts: Vec<String> = self
            .steps
            .iter()
            .map(|s| {
                format!(
                    "{}:n={}:chars≈{}",
                    s.stage.as_str(),
                    s.message_count,
                    s.non_system_chars_est
                )
            })
            .collect();
        parts.join(" | ")
    }
}

/// 每次调用模型前对会话 `messages` 的**同步**变换（工具压缩 → 条数 → 可选字符预算 → 再压缩 → 孤立 tool → 合并 assistant）。
///
/// `report` 为 `Some` 时写入各步快照（建议在 `RUST_LOG` 含 debug 时传入）。
pub fn apply_session_sync_pipeline(
    messages: &mut Vec<Message>,
    cfg: &AgentConfig,
    mut report: Option<&mut MessagePipelineReport>,
) {
    if let Some(r) = report.as_mut() {
        r.record(MessagePipelineStage::SessionSyncStart, messages);
    }

    compress_tool_message_contents(messages, cfg.tool_message_max_chars);
    if let Some(r) = report.as_mut() {
        r.record(MessagePipelineStage::AfterCompressTool, messages);
    }

    trim_messages_by_count(messages, cfg.max_message_history);
    if let Some(r) = report.as_mut() {
        r.record(MessagePipelineStage::AfterTrimByCount, messages);
    }

    if cfg.context_char_budget > 0 {
        trim_messages_by_char_budget(
            messages,
            cfg.context_char_budget,
            cfg.context_min_messages_after_system,
        );
        if let Some(r) = report.as_mut() {
            r.record(MessagePipelineStage::AfterTrimByCharBudget, messages);
        }
        compress_tool_message_contents(messages, cfg.tool_message_max_chars);
        if let Some(r) = report.as_mut() {
            r.record(MessagePipelineStage::AfterSecondCompressTool, messages);
        }
    }

    drop_orphan_tool_messages(messages);
    if let Some(r) = report.as_mut() {
        r.record(MessagePipelineStage::AfterDropOrphanTool, messages);
    }

    crate::types::merge_consecutive_assistants_in_place(messages);
    if let Some(r) = report.as_mut() {
        r.record(MessagePipelineStage::AfterMergeAssistantsInPlace, messages);
    }
}

// ── 供应商出站（ChatRequest.messages）────────────────────────────────────────

/// 从会话切片构造发往 OpenAI 兼容 API 的 `messages`：**跳过** UI 分隔线与长期记忆注入、剥离 `reasoning_content`、再 normalize（合并相邻 assistant 等）。
#[inline]
pub fn conversation_messages_to_vendor_body(messages: &[Message]) -> Vec<Message> {
    crate::types::normalize_messages_for_openai_compatible_request(
        crate::types::messages_for_api_stripping_reasoning_skip_ui_separators(messages),
    )
}

/// 与 [`conversation_messages_to_vendor_body`] 相同，但输入已是「已 strip」的 `Vec`（避免重复遍历），仅做 normalize。
#[inline]
pub fn normalize_stripped_messages_for_vendor_body(messages: Vec<Message>) -> Vec<Message> {
    crate::types::normalize_messages_for_openai_compatible_request(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, ToolCall};

    fn tool_msg(s: &str) -> Message {
        Message {
            role: "tool".to_string(),
            content: Some(s.to_string()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some("1".into()),
        }
    }

    #[test]
    fn compress_tool_truncates() {
        let long = "x".repeat(2000);
        let mut v = vec![tool_msg(&long)];
        compress_tool_message_contents(&mut v, 256);
        let c = v[0].content.as_deref().unwrap();
        assert!(c.starts_with(&"x".repeat(256)));
        assert!(c.contains("截断"));
        assert!(c.chars().count() < long.chars().count());
    }

    #[test]
    fn trim_by_count_keeps_system_and_tail() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("a".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("b".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("c".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        trim_messages_by_count(&mut v, 2);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].role, "system");
        assert_eq!(v[1].content.as_deref(), Some("b"));
        assert_eq!(v[2].content.as_deref(), Some("c"));
    }

    #[test]
    fn trim_by_count_inserts_user_when_tail_would_be_two_assistants() {
        let mut v = vec![
            Message::system_only("s"),
            Message::user_only("old_u"),
            Message {
                role: "assistant".to_string(),
                content: Some("a1".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("a2".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        trim_messages_by_count(&mut v, 2);
        assert_eq!(v.len(), 3);
        assert_eq!(v[1].role, "user");
        assert_eq!(v[1].content.as_deref(), Some("old_u"));
        assert_eq!(v[2].role, "assistant");
    }

    #[test]
    fn char_budget_drops_oldest_after_system() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("aaaaaaaaaa".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("bbbbbbbbbbbbbbbb".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        trim_messages_by_char_budget(&mut v, 6, 1);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].content.as_deref(), Some("bbbbbbbbbbbbbbbb"));
    }

    fn assistant_with_tool_calls() -> Message {
        Message {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_string(),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: "x".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn drop_orphan_tool_removes_leading_tools_after_trim() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            tool_msg("orphan1"),
            tool_msg("orphan2"),
            Message {
                role: "user".to_string(),
                content: Some("last".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        drop_orphan_tool_messages(&mut v);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].role, "user");
        assert_eq!(v[1].content.as_deref(), Some("last"));
    }

    #[test]
    fn drop_orphan_tool_keeps_chain_after_assistant_tool_calls() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            assistant_with_tool_calls(),
            tool_msg("a"),
            tool_msg("b"),
            Message {
                role: "user".to_string(),
                content: Some("u".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        drop_orphan_tool_messages(&mut v);
        assert_eq!(v.len(), 5);
    }

    #[test]
    fn drop_orphan_tool_removes_tool_after_assistant_without_tool_calls() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("text only".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            tool_msg("bad"),
        ];
        drop_orphan_tool_messages(&mut v);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn vendor_body_matches_manual_strip_normalize() {
        let sep = Message::chat_ui_separator(true);
        let a = Message {
            role: "assistant".to_string(),
            content: Some("c".to_string()),
            reasoning_content: Some("r".to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let slice = [Message::user_only("u"), sep, a.clone()];
        let via = conversation_messages_to_vendor_body(&slice);
        let manual = crate::types::normalize_messages_for_openai_compatible_request(
            crate::types::messages_for_api_stripping_reasoning_skip_ui_separators(&slice),
        );
        assert_eq!(via, manual);
    }
}
