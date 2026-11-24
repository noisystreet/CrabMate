//! 回合上下文时间线：从会话缓冲与管道 delta 收集至多两条事件（注入+skill / 窗口操作）。
//!
//! 不发 SSE、不依赖 `execute/tools`。外循环多次 `prepare_*` 只合并回合态。
//!
//! **`detail` 是类型化的**（#1347 S4a）：`ContextTimelineDetail` 取代手写 `serde_json::json!`，
//! 字段集合与类型变成编译期约束；序列化只在 encode 边界发生（SSE / `timeline_marker`），
//! 线上与 canonical 的形状仍是**字符串化 JSON**（S4b 才考虑改成 object）。

use crate::cm_agent::message_pipeline::MessagePipelineDelta;
use crate::cm_types::{
    CRABMATE_CONTEXT_SUMMARY_NAME, CRABMATE_EXECUTION_CONSTRAINT_HINT_NAME,
    CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, CRABMATE_LONG_TERM_MEMORY_NAME,
    CRABMATE_WORKSPACE_CHANGELIST_NAME, Message, is_chat_timeline_marker, message_content_as_str,
    user_message_counts_for_branch_truncation,
};

mod detail;
pub use detail::{ContextTimelineDetail, InjectDetail, SkillRef, TrimDetail};

pub const KIND_CONTEXT_INJECT: &str = "context_inject";
pub const KIND_CONTEXT_TRIM: &str = "context_trim";

const TITLE_INJECT: &str = "本轮已注入上下文";
const TITLE_TRIM: &str = "已裁剪历史";
const TITLE_TOOL_COMPRESS: &str = "已压缩工具输出";

/// 每回合最多保留多少条 `tool` 正文明细，避免一个回合内多次 prepare 让 `detail` 无界增长。
const MAX_TOOL_COMPRESS_DETAIL: usize = 16;

/// 可编码为 `timeline_log` 的一条旁注（kind 已按 §6.1 合并）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextTimelineEvent {
    pub kind: &'static str,
    pub title: String,
    pub detail: ContextTimelineDetail,
}

/// 一次 `prepare_messages_for_model` 完成后的快照（含 changelist sync）。
#[derive(Clone, Copy, Debug)]
pub struct ContextCompactionTimelineSnapshot {
    pub before_tokens: u32,
    pub after_tokens: u32,
    pub max_input_tokens: Option<u32>,
    pub reserved_output_tokens: Option<u32>,
    pub message_tokens: u32,
    pub tool_schema_tokens: u32,
    pub attachment_tokens: u32,
    pub counting_source: Option<&'static str>,
    pub token_triggered: bool,
    pub removed_turn_groups: usize,
    pub removed_messages: usize,
    pub compaction_reason: &'static str,
}

impl Default for ContextCompactionTimelineSnapshot {
    fn default() -> Self {
        Self {
            before_tokens: 0,
            after_tokens: 0,
            max_input_tokens: None,
            reserved_output_tokens: None,
            message_tokens: 0,
            tool_schema_tokens: 0,
            attachment_tokens: 0,
            counting_source: None,
            token_triggered: false,
            removed_turn_groups: 0,
            removed_messages: 0,
            compaction_reason: "none",
        }
    }
}

/// 一次 `prepare_messages_for_model` 完成后的快照（含 changelist sync）。
#[derive(Clone, Debug)]
pub struct ContextTimelineSnapshot<'a> {
    pub messages: &'a [Message],
    pub pipeline: MessagePipelineDelta,
    pub summarized: bool,
    pub summary_tail_kept: Option<usize>,
    pub compaction: ContextCompactionTimelineSnapshot,
}

/// 用户回合内累加；SSE 每种 kind 最多发一次，落盘用最终合并结果。
#[derive(Debug, Default)]
pub struct ContextTimelineAcc {
    inject_kinds: Vec<String>,
    inject_skills: Vec<detail::SkillRef>,
    n_before: Option<usize>,
    n_after: usize,
    count_hit: bool,
    char_hit: bool,
    compress_hits: usize,
    tool_compressed: Vec<crate::cm_agent::message_pipeline::ToolCompressHit>,
    summarized: bool,
    summary_tail_kept: Option<usize>,
    token_before: Option<u32>,
    token_after: Option<u32>,
    max_input_tokens: Option<u32>,
    reserved_output_tokens: Option<u32>,
    message_tokens: Option<u32>,
    tool_schema_tokens: Option<u32>,
    attachment_tokens: Option<u32>,
    counting_source: Option<&'static str>,
    token_triggered: bool,
    removed_turn_groups: usize,
    removed_messages: usize,
    compaction_reason: Option<&'static str>,
    sse_inject_sent: bool,
    sse_trim_sent: bool,
    persist_flushed: bool,
}

/// 追加本档的降级明细；每个用户回合最多留 `MAX_TOOL_COMPRESS_DETAIL` 条，
/// 否则一个回合内多次 prepare 会让 `context_trim` 的 `detail` 无界增长。
fn merge_tool_compress_detail(
    acc: &mut Vec<crate::cm_agent::message_pipeline::ToolCompressHit>,
    hits: &[crate::cm_agent::message_pipeline::ToolCompressHit],
) {
    for hit in hits {
        if acc.len() >= MAX_TOOL_COMPRESS_DETAIL {
            return;
        }
        acc.push(*hit);
    }
}

impl ContextTimelineAcc {
    /// 合并本次 prepare；返回**尚未发过 SSE** 的新事件（每用户回合至多 2 条）。
    pub fn merge(&mut self, snap: ContextTimelineSnapshot<'_>) -> Vec<ContextTimelineEvent> {
        self.merge_injections(snap.messages);
        self.merge_pipeline(snap);
        self.take_new_sse_events()
    }

    fn merge_injections(&mut self, messages: &[Message]) {
        for kind in inject_kinds_in_messages(messages) {
            push_unique(&mut self.inject_kinds, kind);
        }
        for skill in skill_refs_in_messages(messages) {
            if !self.inject_skills.iter().any(|s| s.id == skill.id) {
                self.inject_skills.push(skill);
            }
        }
    }

    fn merge_pipeline(&mut self, snap: ContextTimelineSnapshot<'_>) {
        if self.n_before.is_none() {
            self.n_before = Some(snap.pipeline.n_before);
        }
        self.n_after = snap.pipeline.n_after;
        self.count_hit = self.count_hit || snap.pipeline.trim_count_hit;
        self.char_hit = self.char_hit || snap.pipeline.trim_char_hit;
        self.compress_hits = self
            .compress_hits
            .saturating_add(snap.pipeline.tool_compress_hits);
        merge_tool_compress_detail(&mut self.tool_compressed, &snap.pipeline.tool_compressed);
        if snap.summarized {
            self.summarized = true;
            if snap.summary_tail_kept.is_some() {
                self.summary_tail_kept = snap.summary_tail_kept;
            }
        }
        let compaction = snap.compaction;
        if self.token_before.is_none() || compaction.token_triggered {
            self.token_before = Some(compaction.before_tokens);
        }
        self.token_after = Some(compaction.after_tokens);
        self.message_tokens = Some(compaction.message_tokens);
        self.tool_schema_tokens = Some(compaction.tool_schema_tokens);
        self.attachment_tokens = Some(compaction.attachment_tokens);
        self.counting_source = compaction.counting_source;
        self.max_input_tokens = compaction.max_input_tokens;
        self.reserved_output_tokens = compaction.reserved_output_tokens;
        self.token_triggered = self.token_triggered || compaction.token_triggered;
        self.removed_turn_groups = self
            .removed_turn_groups
            .saturating_add(compaction.removed_turn_groups);
        self.removed_messages = self
            .removed_messages
            .saturating_add(compaction.removed_messages);
        if compaction.compaction_reason != "none" {
            self.compaction_reason = Some(compaction.compaction_reason);
        }
    }

    /// 有注入就发——**包括只有技能注入的回合**。
    ///
    /// #1478 之后这里只看 `kinds`，于是「只用了 `/<id>`、没有其它注入」的回合完全不发
    /// `context_inject`，客户端因此拿不到技能行（crabmate-client#581）。技能也算注入。
    fn has_inject(&self) -> bool {
        !self.inject_kinds.is_empty() || !self.inject_skills.is_empty()
    }

    fn has_trim(&self) -> bool {
        self.count_hit
            || self.char_hit
            || self.compress_hits > 0
            || self.summarized
            || self.removed_turn_groups > 0
    }

    /// 本轮是否真的动过历史（删组 / 摘要 / 条数或字符裁剪）。
    ///
    /// **契约（#1345）**：只有 `true` 时服务端 `used_input_tokens` 才可能低于此前的峰值，
    /// 客户端据此决定要不要把「上下文用量水位」回落；纯工具正文截断（`compress_hits > 0`
    /// 而其余为 0）不改变历史，水位应继续走滚动最大。
    ///
    /// 此前该信息只隐含在 `title` 文案里（`TITLE_TRIM` / `TITLE_TOOL_COMPRESS`），
    /// 消费方按展示文案分流，脆弱且容易被读成「历史被压缩」。
    fn history_removed(&self) -> bool {
        self.count_hit || self.char_hit || self.summarized || self.removed_turn_groups > 0
    }

    fn trim_title(&self) -> &'static str {
        if self.history_removed() {
            TITLE_TRIM
        } else {
            TITLE_TOOL_COMPRESS
        }
    }

    fn take_new_sse_events(&mut self) -> Vec<ContextTimelineEvent> {
        let mut out = Vec::new();
        if self.has_inject() && !self.sse_inject_sent {
            out.push(self.inject_event());
            self.sse_inject_sent = true;
        }
        if self.has_trim() && !self.sse_trim_sent {
            out.push(self.trim_event());
            self.sse_trim_sent = true;
        }
        out
    }

    fn inject_event(&self) -> ContextTimelineEvent {
        ContextTimelineEvent {
            kind: KIND_CONTEXT_INJECT,
            title: TITLE_INJECT.to_string(),
            detail: ContextTimelineDetail::Inject(InjectDetail {
                kinds: self.inject_kinds.clone(),
                skills: self.inject_skills.clone(),
            }),
        }
    }

    fn trim_event(&self) -> ContextTimelineEvent {
        ContextTimelineEvent {
            kind: KIND_CONTEXT_TRIM,
            title: self.trim_title().to_string(),
            // 字段顺序即 JSON 键顺序，与 S4a 之前的 `json!` 逐字一致（见 `detail` 模块文档）。
            detail: ContextTimelineDetail::Trim(TrimDetail {
                count_hit: self.count_hit,
                char_hit: self.char_hit,
                n_before: self.n_before.unwrap_or(0),
                n_after: self.n_after,
                compress_hits: self.compress_hits,
                tool_compressed: self.tool_compressed.clone(),
                summarized: self.summarized,
                history_removed: self.history_removed(),
                tail_kept: self.summary_tail_kept,
                before_tokens: self.token_before,
                after_tokens: self.token_after,
                max_input_tokens: self.max_input_tokens,
                reserved_output_tokens: self.reserved_output_tokens,
                message_tokens: self.message_tokens,
                tool_schema_tokens: self.tool_schema_tokens,
                attachment_tokens: self.attachment_tokens,
                counting_source: self.counting_source.map(str::to_string),
                token_triggered: self.token_triggered,
                removed_turn_groups: self.removed_turn_groups,
                removed_messages: self.removed_messages,
                compaction_reason: self.compaction_reason.map(str::to_string),
            }),
        }
    }

    /// 落盘用 `crabmate_timeline` 行（0～2 条）；可重复调用，第二次为空。
    pub fn persist_markers(&mut self) -> Vec<Message> {
        if self.persist_flushed {
            return Vec::new();
        }
        self.persist_flushed = true;
        let mut out = Vec::new();
        if self.has_inject() {
            out.push(timeline_marker(&self.inject_event()));
        }
        if self.has_trim() {
            out.push(timeline_marker(&self.trim_event()));
        }
        out
    }
}

fn push_unique(dest: &mut Vec<String>, item: String) {
    if !dest.iter().any(|s| s == &item) {
        dest.push(item);
    }
}

/// 把「本轮强制调用的技能」收成结构化引用（`/<id>`）。
///
/// 只认 [`crate::cm_types::is_forced_skill_injection`] 这条**专用注入消息**，并解析它自己生成
/// 的固定标题行（`cm_config::skills::forced_skill_id_from_injection_text`）——不读 `system`、
/// 不读模型输出，那是 #1478 删掉的嗅探路径。
fn skill_refs_in_messages(messages: &[Message]) -> Vec<detail::SkillRef> {
    // 只看「最后一个真实 user 之后」的窗口（与 `turn_completion` 的证据窗口同一先例）：
    // 上一回合的 `/<id>` 注入条若在失败 / 取消路径上没被 strip 掉，全量扫描会把它算进本轮。
    let window = crate::cm_types::messages_slice_since_last_real_user(messages, false)
        .unwrap_or(messages);
    // 每回合至多一条强制注入：只取**最后一条**，天然去重。
    let Some(m) = window
        .iter()
        .rev()
        .find(|m| crate::cm_types::is_forced_skill_injection(m))
    else {
        return Vec::new();
    };
    let Some(text) = m.content.as_ref().and_then(|c| match c {
        crate::cm_types::MessageContent::Text(s) => Some(s.as_str()),
        _ => None,
    }) else {
        return Vec::new();
    };
    let Some(id) = crate::cm_config::skills::forced_skill_id_from_injection_text(text) else {
        return Vec::new();
    };
    vec![detail::SkillRef {
        id,
        source: "forced".to_string(),
    }]
}

fn inject_kinds_in_messages(messages: &[Message]) -> Vec<String> {
    let include_workspace_profile = messages
        .iter()
        .filter(|m| user_message_counts_for_branch_truncation(m))
        .count()
        <= 1;
    let mut kinds = Vec::new();
    for m in messages {
        if m.role != "user" && m.role != "system" {
            continue;
        }
        let Some(name) = m.name.as_deref() else {
            continue;
        };
        let kind = match name {
            CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME if include_workspace_profile => {
                "workspace_profile"
            }
            CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME => continue,
            CRABMATE_LONG_TERM_MEMORY_NAME => "memory",
            CRABMATE_WORKSPACE_CHANGELIST_NAME => "changelist",
            CRABMATE_EXECUTION_CONSTRAINT_HINT_NAME => "execution_constraint",
            CRABMATE_CONTEXT_SUMMARY_NAME => continue,
            _ => continue,
        };
        push_unique(&mut kinds, kind.to_string());
    }
    kinds
}

fn context_window_timeline_kind(m: &Message) -> Option<&str> {
    if !is_chat_timeline_marker(m) {
        return None;
    }
    let body = message_content_as_str(&m.content)?;
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    match v.get("kind").and_then(|k| k.as_str()) {
        Some(KIND_CONTEXT_INJECT) => Some(KIND_CONTEXT_INJECT),
        Some(KIND_CONTEXT_TRIM) => Some(KIND_CONTEXT_TRIM),
        _ => None,
    }
}

#[inline]
pub fn is_context_window_timeline_marker(message: &Message) -> bool {
    context_window_timeline_kind(message).is_some()
}

/// 去掉本会话已落盘的 `context_inject` / `context_trim` 旁注（其它 `crabmate_timeline` 保留）。
pub fn strip_context_window_timeline_markers(messages: &mut Vec<Message>) {
    messages.retain(|m| context_window_timeline_kind(m).is_none());
}

pub fn timeline_marker(event: &ContextTimelineEvent) -> Message {
    // `detail` 仍写成**字符串**：canonical 与 SSE 同形状（S4a 不改 wire，见 `detail` 模块文档）。
    let content = serde_json::json!({
        "kind": event.kind,
        "title": event.title,
        "detail": event.detail.to_json_string(),
    })
    .to_string();
    Message {
        role: "system".to_string(),
        content: Some(crate::cm_types::MessageContent::Text(content)),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: Some("crabmate_timeline".to_string()),
        tool_call_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::{Message, MessageContent};

    fn named_user(name: &str, text: &str) -> Message {
        Message {
            role: "user".into(),
            content: Some(MessageContent::Text(text.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(name.into()),
            tool_call_id: None,
        }
    }

    #[test]
    fn no_change_emits_nothing() {
        let mut acc = ContextTimelineAcc::default();
        let msgs = vec![Message::system_only("s"), Message::user_only("hi")];
        let ev = acc.merge(ContextTimelineSnapshot {
            messages: &msgs,
            pipeline: MessagePipelineDelta::default(),
            summarized: false,
            summary_tail_kept: None,
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        assert!(ev.is_empty());
        assert!(acc.persist_markers().is_empty());
    }

    #[test]
    fn tool_compress_only_does_not_claim_history_was_trimmed() {
        let mut acc = ContextTimelineAcc::default();
        let msgs = vec![Message::system_only("s"), Message::user_only("hi")];
        let events = acc.merge(ContextTimelineSnapshot {
            messages: &msgs,
            pipeline: MessagePipelineDelta {
                n_before: 2,
                n_after: 2,
                trim_count_hit: false,
                trim_char_hit: false,
                tool_compress_hits: 1,
                tool_compressed: vec![crate::cm_agent::message_pipeline::ToolCompressHit {
                    index: 2,
                    original_chars: 40_000,
                    kept_chars: 32_768,
                }],
                ..MessagePipelineDelta::default()
            },
            summarized: false,
            summary_tail_kept: None,
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, KIND_CONTEXT_TRIM);
        assert_eq!(events[0].title, TITLE_TOOL_COMPRESS);
        let trim = events[0].detail.as_trim().expect("trim detail");
        assert_eq!(trim.compress_hits, 1);
        assert_eq!(
            trim.tool_compressed,
            vec![crate::cm_agent::message_pipeline::ToolCompressHit {
                index: 2,
                original_chars: 40_000,
                kept_chars: 32_768,
            }],
            "detail 应带被压条目明细"
        );
        assert!(
            !trim.history_removed,
            "纯工具截断不得声称动过历史（#1345）"
        );
    }

    /// #1345：`history_removed` 是四条「真动历史」路径的并集，任一条成立即 `true`。
    ///
    /// 客户端只在这个字段为 `true` 时回落用量水位，所以它必须覆盖全部四种删/压路径——
    /// 漏掉任何一种，状态栏就会停在旧峰值（与工具截断那次故障相反方向的 bug）。
    #[test]
    fn history_removed_covers_every_real_trim_path() {
        let msgs = vec![Message::system_only("s"), Message::user_only("hi")];
        let cases: [(&str, MessagePipelineDelta, bool, ContextCompactionTimelineSnapshot); 4] = [
            (
                "count_hit",
                MessagePipelineDelta { trim_count_hit: true, ..Default::default() },
                false,
                ContextCompactionTimelineSnapshot::default(),
            ),
            (
                "char_hit",
                MessagePipelineDelta { trim_char_hit: true, ..Default::default() },
                false,
                ContextCompactionTimelineSnapshot::default(),
            ),
            (
                "summarized",
                MessagePipelineDelta::default(),
                true,
                ContextCompactionTimelineSnapshot::default(),
            ),
            (
                "removed_turn_groups",
                MessagePipelineDelta::default(),
                false,
                ContextCompactionTimelineSnapshot { removed_turn_groups: 2, ..Default::default() },
            ),
        ];
        for (label, pipeline, summarized, compaction) in cases {
            let mut acc = ContextTimelineAcc::default();
            let events = acc.merge(ContextTimelineSnapshot {
                messages: &msgs,
                pipeline,
                summarized,
                summary_tail_kept: summarized.then_some(6),
                compaction,
            });
            let ev = events.first().unwrap_or_else(|| panic!("{label} 应产生事件"));
            assert_eq!(ev.kind, KIND_CONTEXT_TRIM, "{label}");
            assert_eq!(ev.title, TITLE_TRIM, "{label} 应展示为真裁剪");
            assert!(
                ev.detail.as_trim().expect("trim detail").history_removed,
                "{label} 的 detail 应标出 history_removed=true"
            );
        }
    }

    #[test]
    fn token_group_compaction_reports_soft_budget_fields() {
        let mut acc = ContextTimelineAcc::default();
        let msgs = vec![Message::system_only("s"), Message::user_only("latest")];
        let events = acc.merge(ContextTimelineSnapshot {
            messages: &msgs,
            pipeline: MessagePipelineDelta {
                n_before: 8,
                n_after: 2,
                ..MessagePipelineDelta::default()
            },
            summarized: false,
            summary_tail_kept: None,
            compaction: ContextCompactionTimelineSnapshot {
                before_tokens: 9_000,
                after_tokens: 6_000,
                max_input_tokens: Some(8_000),
                reserved_output_tokens: Some(2_000),
                message_tokens: 5_000,
                tool_schema_tokens: 900,
                attachment_tokens: 100,
                counting_source: Some("matched_tokenizer"),
                token_triggered: true,
                removed_turn_groups: 2,
                removed_messages: 6,
                compaction_reason: "token_budget_turn_groups",
            },
        });
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, TITLE_TRIM);
        let trim = events[0].detail.as_trim().expect("trim detail");
        assert_eq!(trim.before_tokens, Some(9_000));
        assert_eq!(
            trim.compaction_reason.as_deref(),
            Some("token_budget_turn_groups")
        );
    }

    #[test]
    fn inject_and_trim_merge_to_at_most_two_sse() {
        let mut acc = ContextTimelineAcc::default();
        let msgs = vec![
            Message::system_only("s"),
            named_user(CRABMATE_LONG_TERM_MEMORY_NAME, "mem"),
            named_user(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, "ws"),
        ];
        let first = acc.merge(ContextTimelineSnapshot {
            messages: &msgs,
            pipeline: MessagePipelineDelta {
                n_before: 10,
                n_after: 8,
                trim_count_hit: true,
                trim_char_hit: false,
                tool_compress_hits: 2,
                ..MessagePipelineDelta::default()
            },
            summarized: true,
            summary_tail_kept: Some(6),
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].kind, KIND_CONTEXT_INJECT);
        assert_eq!(first[1].kind, KIND_CONTEXT_TRIM);
        assert_eq!(first[1].title, TITLE_TRIM);
        let inject = first[0].detail.as_inject().expect("inject detail");
        assert!(inject.kinds.iter().any(|k| k == "memory"));
        assert!(first[1].detail.as_trim().expect("trim detail").summarized);
        let second = acc.merge(ContextTimelineSnapshot {
            messages: &msgs,
            pipeline: MessagePipelineDelta {
                n_before: 8,
                n_after: 7,
                trim_count_hit: true,
                trim_char_hit: true,
                tool_compress_hits: 1,
                ..MessagePipelineDelta::default()
            },
            summarized: false,
            summary_tail_kept: None,
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        assert!(second.is_empty());
        let persisted = acc.persist_markers();
        assert_eq!(persisted.len(), 2);
        assert!(persisted.iter().all(is_chat_timeline_marker));
        let mut roundtrip = msgs.clone();
        roundtrip.extend(persisted.clone());
        let snap = crate::cm_types::filter_messages_for_web_client_snapshot(&roundtrip);
        assert!(snap.iter().any(is_chat_timeline_marker));
        let vendor = crate::cm_types::messages_for_api_stripping_reasoning_skip_ui_separators(
            &roundtrip, false, false,
        );
        assert!(vendor.iter().all(|m| !is_chat_timeline_marker(m)));
        assert!(acc.persist_markers().is_empty());
    }

    #[test]
    fn workspace_profile_only_on_first_counting_user_turn() {
        let profile = named_user(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, "ws");
        let mem = named_user(CRABMATE_LONG_TERM_MEMORY_NAME, "mem");
        let first = vec![
            Message::system_only("s"),
            profile.clone(),
            mem.clone(),
            Message::user_only("hi"),
        ];
        assert!(
            inject_kinds_in_messages(&first)
                .iter()
                .any(|k| k == "workspace_profile")
        );
        let later = vec![
            Message::system_only("s"),
            profile,
            mem,
            Message::user_only("hi"),
            Message::user_only("again"),
        ];
        let kinds = inject_kinds_in_messages(&later);
        assert!(!kinds.iter().any(|k| k == "workspace_profile"));
        assert!(kinds.iter().any(|k| k == "memory"));
    }

    #[test]
    fn strip_replaces_previous_context_window_markers() {
        let mut msgs = vec![Message::system_only("s"), Message::user_only("hi")];
        let mut acc = ContextTimelineAcc::default();
        let with_mem = vec![
            Message::system_only("s"),
            named_user(CRABMATE_LONG_TERM_MEMORY_NAME, "mem"),
            Message::user_only("hi"),
        ];
        acc.merge(ContextTimelineSnapshot {
            messages: &with_mem,
            pipeline: MessagePipelineDelta::default(),
            summarized: false,
            summary_tail_kept: None,
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        msgs.extend(acc.persist_markers());
        assert_eq!(
            msgs.iter().filter(|m| is_chat_timeline_marker(m)).count(),
            1
        );
        let mut acc2 = ContextTimelineAcc::default();
        acc2.merge(ContextTimelineSnapshot {
            messages: &with_mem,
            pipeline: MessagePipelineDelta {
                n_before: 9,
                n_after: 7,
                trim_count_hit: true,
                trim_char_hit: false,
                tool_compress_hits: 0,
                ..MessagePipelineDelta::default()
            },
            summarized: false,
            summary_tail_kept: None,
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        strip_context_window_timeline_markers(&mut msgs);
        msgs.extend(acc2.persist_markers());
        let markers: Vec<_> = msgs.iter().filter(|m| is_chat_timeline_marker(m)).collect();
        assert_eq!(markers.len(), 2);
        assert!(markers.iter().any(|m| context_window_timeline_kind(m)
            == Some(KIND_CONTEXT_INJECT)));
        assert!(markers
            .iter()
            .any(|m| context_window_timeline_kind(m) == Some(KIND_CONTEXT_TRIM)));
        let snap = crate::cm_types::filter_messages_for_web_client_snapshot(&msgs);
        assert_eq!(
            snap.iter().filter(|m| is_chat_timeline_marker(m)).count(),
            2
        );
    }

    /// 旧 canonical marker 的 `detail` 是字符串化 JSON（S4a 不改写历史数据）：
    /// 读侧必须能从**最老形状**解析出 `TrimDetail`，缺字段按默认值补而不是解析失败。
    #[test]
    fn legacy_detail_string_parses_into_trim_detail() {
        let legacy = r#"{"count_hit":true,"n_before":12,"n_after":8}"#;
        let detail: ContextTimelineDetail =
            serde_json::from_str(legacy).expect("parse legacy detail");
        let trim = detail.as_trim().expect("trim detail");
        assert!(trim.count_hit);
        assert_eq!(trim.n_before, 12);
        assert_eq!(trim.n_after, 8);
        assert!(trim.tool_compressed.is_empty(), "缺字段按默认值补");
        assert_eq!(trim.after_tokens, None);
        assert_eq!(trim.compaction_reason, None);
    }

    /// 判别必须按**形状**分派（review 补）：`untagged` + 全字段 `default` 的 struct 若允许
    /// 未知字段，`{"kinds":…}` 会被 `Trim` 先吞成 `Trim(default)`，`Inject` 分支永不命中。
    /// 这条测两个方向都能落到正确变体——它是 `deny_unknown_fields` 的回归守卫。
    #[test]
    fn detail_json_dispatches_by_shape() {
        let inject_json = r#"{"kinds":["memory"]}"#;
        let detail: ContextTimelineDetail =
            serde_json::from_str(inject_json).expect("parse inject detail");
        assert!(
            detail.as_inject().is_some(),
            "inject 形状不得被 Trim 吞掉: {detail:?}"
        );

        let trim_json = r#"{"count_hit":true,"n_before":12}"#;
        let detail: ContextTimelineDetail =
            serde_json::from_str(trim_json).expect("parse trim detail");
        assert!(
            detail.as_trim().is_some(),
            "trim 形状不得被 Inject 吞掉: {detail:?}"
        );
    }

    /// #1347 S4a：`detail` 类型化后，序列化字节必须与类型化之前的 `json!` 实现**逐字相同**。
    ///
    /// 守的是 wire 形状本身：`detail` 在线上与 canonical 里仍是字符串化 JSON，
    /// 键改名 / 换序 / 漏字段都会让新老数据出现两种形态，而线上的字节差异在 Client 侧
    /// 通常是「某个字段读不到」这种沉默失败。夹具即类型化前那份实现的实拍输出。
    #[test]
    fn detail_json_bytes_match_golden() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/context_timeline_detail_golden.json"
        ))
        .expect("parse fixtures/context_timeline_detail_golden.json");

        // trim：全部字段都有值的那一档（最容易被漏掉的是 `null` 的位置与键顺序）。
        let msgs = vec![Message::system_only("s"), Message::user_only("latest")];
        let mut acc = ContextTimelineAcc::default();
        let events = acc.merge(ContextTimelineSnapshot {
            messages: &msgs,
            pipeline: MessagePipelineDelta {
                n_before: 8,
                n_after: 2,
                trim_count_hit: true,
                tool_compress_hits: 1,
                tool_compressed: vec![crate::cm_agent::message_pipeline::ToolCompressHit {
                    index: 3,
                    original_chars: 40_000,
                    kept_chars: 2_000,
                }],
                ..MessagePipelineDelta::default()
            },
            summarized: true,
            summary_tail_kept: Some(6),
            compaction: ContextCompactionTimelineSnapshot {
                before_tokens: 9_000,
                after_tokens: 6_000,
                max_input_tokens: Some(8_000),
                reserved_output_tokens: Some(2_000),
                message_tokens: 5_000,
                tool_schema_tokens: 900,
                attachment_tokens: 100,
                counting_source: Some("matched_tokenizer"),
                token_triggered: true,
                removed_turn_groups: 2,
                removed_messages: 6,
                compaction_reason: "token_budget_turn_groups",
            },
        });
        let trim = events
            .iter()
            .find(|e| e.kind == KIND_CONTEXT_TRIM)
            .expect("trim event");
        assert_eq!(
            trim.detail.to_json_string(),
            fixture["trim_detail"].as_str().unwrap_or("<missing>"),
            "trim detail 的字节形状必须与类型化前一致"
        );

        // inject：只剩 `kinds`（技能统计已删除；`skills`/`forced` 是历史兼容壳、不再序列化）。
        let msgs = vec![
            Message::system_only("s"),
            named_user(CRABMATE_LONG_TERM_MEMORY_NAME, "mem"),
        ];
        let mut acc = ContextTimelineAcc::default();
        let events = acc.merge(ContextTimelineSnapshot {
            messages: &msgs,
            pipeline: MessagePipelineDelta::default(),
            summarized: false,
            summary_tail_kept: None,
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        let inject = events
            .iter()
            .find(|e| e.kind == KIND_CONTEXT_INJECT)
            .expect("inject event");
        assert_eq!(
            inject.detail.to_json_string(),
            fixture["inject_detail"].as_str().unwrap_or("<missing>"),
            "inject detail 的字节形状必须与类型化前一致"
        );
    }

    /// 只有 `/<id>` 注入、没有任何 `kinds` 的回合**也必须发** `context_inject`（crabmate-client#581）。
    ///
    /// #1478 之后 `has_inject` 只看 `kinds`，这类回合一条事件都不发，客户端因此永远拿不到技能行。
    #[test]
    fn forced_skill_only_turn_emits_inject_event_with_skills() {
        let appendix = format!(
            "{}rust-style）】\n以下内容由用户通过斜杠命令强制注入；…\n",
            crate::cm_config::skills::SKILLS_FORCED_TITLE_PREFIX
        );
        let msgs = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            Message::user_forced_skill_injection(appendix),
        ];
        let mut acc = ContextTimelineAcc::default();
        let events = acc.merge(ContextTimelineSnapshot {
            messages: &msgs,
            pipeline: MessagePipelineDelta::default(),
            summarized: false,
            summary_tail_kept: None,
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        let inject = events
            .iter()
            .find(|e| e.kind == KIND_CONTEXT_INJECT)
            .expect("只有技能注入也必须发 context_inject");
        assert_eq!(
            inject.detail.to_json_string(),
            r#"{"kinds":[],"skills":[{"id":"rust-style","source":"forced"}]}"#
        );
    }

    /// 没有 `/<id>` 时 `detail` 不得冒出 `skills` 键——线下字节与 #1478 之后的形态一致。
    #[test]
    fn inject_detail_omits_skills_key_when_not_forced() {
        let detail = ContextTimelineDetail::Inject(InjectDetail {
            kinds: vec!["memory".to_string()],
            skills: Vec::new(),
        });
        assert_eq!(detail.to_json_string(), r#"{"kinds":["memory"]}"#);
    }

    /// 旧 `detail`（只有 `kinds`）仍可反序列化：`skills` 走 `default` 落空（过渡期线上旧客户端/旧落盘）。
    #[test]
    fn legacy_inject_detail_deserializes_without_skills() {
        let d: ContextTimelineDetail =
            serde_json::from_str(r#"{"kinds":["memory"]}"#).expect("legacy detail 必须可读");
        assert_eq!(d.as_inject().map(|i| i.skills.len()), Some(0));
    }
}
