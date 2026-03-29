//! 对话 **`Message` 变换管道**：统一「会话内存侧」与「发往供应商」两阶段的处理顺序与可观测性。
//!
//! ## 两阶段
//!
//! 1. **会话同步（`apply_session_sync_pipeline`）**：在每次调用模型前对**进程内** `Vec<Message>` 就地处理——工具正文压缩（`crabmate_tool` 信封内 **`output`** 超长时首尾采样 + 元数据，见 [`crate::tool_result::maybe_compress_tool_message_content`]）、条数/字符裁剪、孤立 `tool` 剔除、合并相邻 `assistant`（保留会话尾部空占位语义，见 [`crate::types::normalize_messages_for_openai_compatible_request`] 文档）。实现原在 [`super::context_window`]，现经本模块编排。
//! 2. **供应商出站（`conversation_messages_to_vendor_body` 等）**：从会话切片构造 **`ChatRequest.messages`**：跳过 UI 分隔线与长期记忆注入、按网关策略去掉 `reasoning_content`（Moonshot **kimi-k2.5** 在 thinking 启用时对含 **`tool_calls`** 的 assistant **保留**思维链，见 [`crate::llm::kimi_k2_5_vendor_requires_tool_call_reasoning`]）、再经 OpenAI 兼容 normalize（合并相邻 assistant、清理尾部非法 assistant）；若 **`llm_fold_system_into_user`** 为真（见配置；接 MiniMax 等时常需开启），再将 **`system`** 折叠进后续 **`user`**。**不**写入会话 `Vec`。
//!
//! ## 会话同步顺序契约（勿打乱）
//!
//! 对 [`apply_session_sync_pipeline`] / [`apply_session_sync_pipeline_with_config`] 中**固定**为：
//!
//! 1. 记录起点快照（`SessionSyncStart`）
//! 2. **`compress_tool_message_contents`**（`tool_message_max_chars`）
//! 3. **`trim_messages_by_count`**（`max_message_history`）
//! 4. 若 **`context_char_budget > 0`**：`trim_messages_by_char_budget` → 再次 **`compress_tool_message_contents`**
//! 5. **`drop_orphan_tool_messages`**
//! 6. **`merge_consecutive_assistants_in_place`**
//!
//! 新增步骤须同步更新 [`MessagePipelineStage`]、本列表、以及 `docs/DEVELOPMENT.md` 中上下文策略描述。
//!
//! ## 可观测性
//!
//! - **Debug**：`context_window` 在 `RUST_LOG` 含 debug 时打一行汇总（`message_pipeline session_sync: …`）。
//! - **Trace**：`target=crabmate::message_pipeline`，每步一行 `session_sync_step stage=… message_count=… non_system_chars_est=…`（便于 grep/采集）；设置 `RUST_LOG=crabmate::message_pipeline=trace`。
//!
//! 新增处理步骤时：优先在本文件增加 `MessagePipelineStage` 变体，并在 `apply_session_sync_pipeline` 中按固定顺序调用，避免在 `agent_turn` / `llm` 多处散落。

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::AgentConfig;
use crate::types::Message;

/// 进程内累计：每次 `prepare_messages_for_model` 内同步管道实际发生裁剪/剔除时递增（供 `GET /status` 排障）。
#[derive(Debug, Default)]
pub struct MessagePipelineCounters {
    pub trim_count_hits: AtomicU64,
    pub trim_char_budget_hits: AtomicU64,
    pub tool_compress_hits: AtomicU64,
    pub orphan_tool_drops: AtomicU64,
}

impl MessagePipelineCounters {
    pub fn snapshot(&self) -> MessagePipelineCountersSnapshot {
        MessagePipelineCountersSnapshot {
            trim_count_hits: self.trim_count_hits.load(Ordering::Relaxed),
            trim_char_budget_hits: self.trim_char_budget_hits.load(Ordering::Relaxed),
            tool_compress_hits: self.tool_compress_hits.load(Ordering::Relaxed),
            orphan_tool_drops: self.orphan_tool_drops.load(Ordering::Relaxed),
        }
    }
}

/// `MessagePipelineCounters::snapshot()` 的纯数据副本（可序列化到 `/status`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct MessagePipelineCountersSnapshot {
    pub trim_count_hits: u64,
    pub trim_char_budget_hits: u64,
    pub tool_compress_hits: u64,
    pub orphan_tool_drops: u64,
}

/// 全局计数器（Web 与 CLI 共用同一进程内实例）。
pub static MESSAGE_PIPELINE_COUNTERS: LazyLock<MessagePipelineCounters> =
    LazyLock::new(MessagePipelineCounters::default);

/// 会话同步管道所用配置子集（便于测试与不依赖完整 [`AgentConfig`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagePipelineConfig {
    pub tool_message_max_chars: usize,
    pub max_message_history: usize,
    pub context_char_budget: usize,
    pub context_min_messages_after_system: usize,
}

impl From<&AgentConfig> for MessagePipelineConfig {
    fn from(cfg: &AgentConfig) -> Self {
        Self {
            tool_message_max_chars: cfg.tool_message_max_chars,
            max_message_history: cfg.max_message_history,
            context_char_budget: cfg.context_char_budget,
            context_min_messages_after_system: cfg.context_min_messages_after_system,
        }
    }
}

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

/// 截断 `tool` 消息正文（过长时追加说明尾注）；返回本轮压缩的 `tool` 条数。
pub fn compress_tool_message_contents(messages: &mut [Message], max_chars: usize) -> usize {
    let max_chars = max_chars.max(256);
    let mut n = 0usize;
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

    /// 单行摘要，便于 `log` debug 汇总。
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

fn log_session_sync_step(stage: MessagePipelineStage, messages: &[Message]) {
    log::trace!(
        target: "crabmate::message_pipeline",
        "session_sync_step stage={} message_count={} non_system_chars_est={}",
        stage.as_str(),
        messages.len(),
        estimate_non_system_chars(messages),
    );
}

fn record_and_trace(
    report: &mut Option<&mut MessagePipelineReport>,
    stage: MessagePipelineStage,
    messages: &[Message],
) {
    if let Some(r) = report.as_mut() {
        r.record(stage, messages);
    }
    log_session_sync_step(stage, messages);
}

/// 每次调用模型前对会话 `messages` 的**同步**变换；配置为 [`MessagePipelineConfig`]（单测可构造轻量配置）。
///
/// `report` 为 `Some` 时写入各步快照（`context_window` 在 debug 时传入）；**任意** `RUST_LOG` 含 **`crabmate::message_pipeline=trace`** 时每步另打结构化 trace。
pub fn apply_session_sync_pipeline_with_config(
    messages: &mut Vec<Message>,
    cfg: MessagePipelineConfig,
    mut report: Option<&mut MessagePipelineReport>,
) {
    let ctr = &*MESSAGE_PIPELINE_COUNTERS;

    record_and_trace(
        &mut report,
        MessagePipelineStage::SessionSyncStart,
        messages,
    );

    let c1 = compress_tool_message_contents(messages, cfg.tool_message_max_chars);
    if c1 > 0 {
        ctr.tool_compress_hits
            .fetch_add(c1 as u64, Ordering::Relaxed);
    }
    record_and_trace(
        &mut report,
        MessagePipelineStage::AfterCompressTool,
        messages,
    );

    if trim_messages_by_count(messages, cfg.max_message_history) {
        ctr.trim_count_hits.fetch_add(1, Ordering::Relaxed);
    }
    record_and_trace(
        &mut report,
        MessagePipelineStage::AfterTrimByCount,
        messages,
    );

    if cfg.context_char_budget > 0 {
        if trim_messages_by_char_budget(
            messages,
            cfg.context_char_budget,
            cfg.context_min_messages_after_system,
        ) {
            ctr.trim_char_budget_hits.fetch_add(1, Ordering::Relaxed);
        }
        record_and_trace(
            &mut report,
            MessagePipelineStage::AfterTrimByCharBudget,
            messages,
        );
        let c2 = compress_tool_message_contents(messages, cfg.tool_message_max_chars);
        if c2 > 0 {
            ctr.tool_compress_hits
                .fetch_add(c2 as u64, Ordering::Relaxed);
        }
        record_and_trace(
            &mut report,
            MessagePipelineStage::AfterSecondCompressTool,
            messages,
        );
    }

    let dropped = drop_orphan_tool_messages(messages);
    if dropped > 0 {
        ctr.orphan_tool_drops
            .fetch_add(dropped as u64, Ordering::Relaxed);
    }
    record_and_trace(
        &mut report,
        MessagePipelineStage::AfterDropOrphanTool,
        messages,
    );

    crate::types::merge_consecutive_assistants_in_place(messages);
    record_and_trace(
        &mut report,
        MessagePipelineStage::AfterMergeAssistantsInPlace,
        messages,
    );
}

/// 与 [`apply_session_sync_pipeline_with_config`] 相同，从完整 [`AgentConfig`] 取子集。
pub fn apply_session_sync_pipeline(
    messages: &mut Vec<Message>,
    cfg: &AgentConfig,
    report: Option<&mut MessagePipelineReport>,
) {
    apply_session_sync_pipeline_with_config(messages, MessagePipelineConfig::from(cfg), report);
}

// ── 供应商出站（ChatRequest.messages）────────────────────────────────────────

fn sanitize_assistant_tool_call_arguments_for_vendor_in_place(msgs: &mut [Message]) {
    use crate::types::sanitize_tool_call_arguments_for_openai_compat;

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
                    "tool_calls.function.arguments 非合法 JSON，已替换为 {{}} 以满足上游校验 tool_call_id={} preview={}",
                    tc.id,
                    crate::redact::preview_chars(orig, 80)
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

/// 从会话切片构造发往 OpenAI 兼容 API 的 `messages`：**跳过** UI 分隔线与长期记忆注入、按 `preserve_reasoning_on_assistant_tool_calls` 剥离或保留 `reasoning_content`、再 normalize（合并相邻 assistant 等）；`fold_system_into_user` 为真时再 [`crate::types::fold_system_messages_into_following_user`]。
#[inline]
pub fn conversation_messages_to_vendor_body(
    messages: &[Message],
    fold_system_into_user: bool,
    preserve_reasoning_on_assistant_tool_calls: bool,
) -> Vec<Message> {
    let mut v = crate::types::normalize_messages_for_openai_compatible_request(
        crate::types::messages_for_api_stripping_reasoning_skip_ui_separators(
            messages,
            preserve_reasoning_on_assistant_tool_calls,
        ),
    );
    if fold_system_into_user {
        v = crate::types::fold_system_messages_into_following_user(v);
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
    let mut v = crate::types::normalize_messages_for_openai_compatible_request(messages);
    if fold_system_into_user {
        v = crate::types::fold_system_messages_into_following_user(v);
    }
    sanitize_assistant_tool_call_arguments_for_vendor_in_place(&mut v);
    v
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
            reasoning_details: None,
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
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("a".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("b".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("c".into()),
                reasoning_content: None,
                reasoning_details: None,
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
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("a2".into()),
                reasoning_content: None,
                reasoning_details: None,
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
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("aaaaaaaaaa".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("bbbbbbbbbbbbbbbb".into()),
                reasoning_content: None,
                reasoning_details: None,
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
            reasoning_details: None,
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
                reasoning_details: None,
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
                reasoning_details: None,
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
                reasoning_details: None,
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
                reasoning_details: None,
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
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("text only".into()),
                reasoning_content: None,
                reasoning_details: None,
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
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let slice = [Message::user_only("u"), sep, a.clone()];
        let via = conversation_messages_to_vendor_body(&slice, false, false);
        let manual = crate::types::normalize_messages_for_openai_compatible_request(
            crate::types::messages_for_api_stripping_reasoning_skip_ui_separators(&slice, false),
        );
        assert_eq!(via, manual);
    }

    #[test]
    fn pipeline_report_skips_char_budget_stages_when_budget_zero() {
        let mut v = vec![
            Message::system_only("s"),
            Message::user_only("a"),
            Message::user_only("b"),
        ];
        let cfg = MessagePipelineConfig {
            tool_message_max_chars: 512,
            max_message_history: 10,
            context_char_budget: 0,
            context_min_messages_after_system: 1,
        };
        let mut report = MessagePipelineReport::default();
        apply_session_sync_pipeline_with_config(&mut v, cfg, Some(&mut report));
        let stages: Vec<MessagePipelineStage> = report.steps.iter().map(|s| s.stage).collect();
        assert!(
            !stages.contains(&MessagePipelineStage::AfterTrimByCharBudget),
            "budget=0 不应出现 AfterTrimByCharBudget: {:?}",
            stages
        );
        assert!(
            !stages.contains(&MessagePipelineStage::AfterSecondCompressTool),
            "budget=0 不应出现 AfterSecondCompressTool: {:?}",
            stages
        );
        assert_eq!(
            stages.first(),
            Some(&MessagePipelineStage::SessionSyncStart)
        );
        assert_eq!(
            stages.last(),
            Some(&MessagePipelineStage::AfterMergeAssistantsInPlace)
        );
    }

    #[test]
    fn pipeline_report_includes_char_trim_and_second_compress_when_budget_positive() {
        let mut v = vec![
            Message::system_only("s"),
            Message::user_only("x".repeat(100)),
            Message::user_only("y".repeat(100)),
        ];
        let cfg = MessagePipelineConfig {
            tool_message_max_chars: 512,
            max_message_history: 10,
            context_char_budget: 50,
            context_min_messages_after_system: 1,
        };
        let mut report = MessagePipelineReport::default();
        apply_session_sync_pipeline_with_config(&mut v, cfg, Some(&mut report));
        let stages: Vec<MessagePipelineStage> = report.steps.iter().map(|s| s.stage).collect();
        assert!(
            stages.contains(&MessagePipelineStage::AfterTrimByCharBudget),
            "budget>0 且超长时应出现 AfterTrimByCharBudget: {:?}",
            stages
        );
        assert!(
            stages.contains(&MessagePipelineStage::AfterSecondCompressTool),
            "budget>0 时应出现第二次 compress 阶段: {:?}",
            stages
        );
    }

    #[test]
    fn drop_orphan_after_trim_count_in_full_pipeline() {
        // 条数裁剪后尾部以 `tool`+`user` 开头，前面的 `assistant+tool_calls` 被裁掉 → 孤立 tool 须由管道剔除。
        let mut v = vec![
            Message::system_only("s"),
            Message::user_only("old"),
            assistant_with_tool_calls(),
            tool_msg("t1"),
            Message::user_only("last"),
        ];
        let cfg = MessagePipelineConfig {
            tool_message_max_chars: 512,
            max_message_history: 2,
            context_char_budget: 0,
            context_min_messages_after_system: 1,
        };
        apply_session_sync_pipeline_with_config(&mut v, cfg, None);
        assert!(
            !v.iter().any(|m| m.role == "tool"),
            "trim 后 tool 无有效前驱时应被 drop_orphan 剔除: {:?}",
            v.iter().map(|m| m.role.as_str()).collect::<Vec<_>>()
        );
        assert!(
            v.iter()
                .any(|m| m.role == "user" && m.content.as_deref() == Some("last")),
            "应保留尾部 user: {:?}",
            v
        );
    }
}
