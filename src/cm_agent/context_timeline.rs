//! 回合上下文时间线：从会话缓冲与管道 delta 收集至多两条事件（注入+skill / 窗口操作）。
//!
//! 不发 SSE、不依赖 `execute/tools`。外循环多次 `prepare_*` 只合并回合态。

use crate::cm_agent::message_pipeline::MessagePipelineDelta;
use crate::cm_types::{
    CRABMATE_CONTEXT_SUMMARY_NAME, CRABMATE_EXECUTION_CONSTRAINT_HINT_NAME,
    CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, CRABMATE_LONG_TERM_MEMORY_NAME,
    CRABMATE_WORKSPACE_CHANGELIST_NAME, Message, is_chat_timeline_marker, message_content_as_str,
    user_message_counts_for_branch_truncation,
};

pub const KIND_CONTEXT_INJECT: &str = "context_inject";
pub const KIND_CONTEXT_TRIM: &str = "context_trim";

const TITLE_INJECT: &str = "本轮已注入上下文";
const TITLE_TRIM: &str = "已裁剪历史";
const TITLE_TOOL_COMPRESS: &str = "已压缩工具输出";

/// 可编码为 `timeline_log` 的一条旁注（kind 已按 §6.1 合并）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextTimelineEvent {
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
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
#[derive(Clone, Copy, Debug)]
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
    skill_ids: Vec<String>,
    skill_forced: bool,
    n_before: Option<usize>,
    n_after: usize,
    count_hit: bool,
    char_hit: bool,
    compress_hits: usize,
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
        let skills = skill_hint_from_messages(messages);
        for id in skills.ids {
            push_unique(&mut self.skill_ids, id);
        }
        self.skill_forced = self.skill_forced || skills.forced;
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

    fn has_inject(&self) -> bool {
        !self.inject_kinds.is_empty() || !self.skill_ids.is_empty() || self.skill_forced
    }

    fn has_trim(&self) -> bool {
        self.count_hit
            || self.char_hit
            || self.compress_hits > 0
            || self.summarized
            || self.removed_turn_groups > 0
    }

    fn trim_title(&self) -> &'static str {
        if self.count_hit || self.char_hit || self.summarized || self.removed_turn_groups > 0 {
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
        let detail = serde_json::json!({
            "kinds": self.inject_kinds,
            "skills": self.skill_ids,
            "forced": self.skill_forced,
        })
        .to_string();
        ContextTimelineEvent {
            kind: KIND_CONTEXT_INJECT,
            title: TITLE_INJECT.to_string(),
            detail,
        }
    }

    fn trim_event(&self) -> ContextTimelineEvent {
        let detail = serde_json::json!({
            "count_hit": self.count_hit,
            "char_hit": self.char_hit,
            "n_before": self.n_before.unwrap_or(0),
            "n_after": self.n_after,
            "compress_hits": self.compress_hits,
            "summarized": self.summarized,
            "tail_kept": self.summary_tail_kept,
            "before_tokens": self.token_before,
            "after_tokens": self.token_after,
            "max_input_tokens": self.max_input_tokens,
            "reserved_output_tokens": self.reserved_output_tokens,
            "message_tokens": self.message_tokens,
            "tool_schema_tokens": self.tool_schema_tokens,
            "attachment_tokens": self.attachment_tokens,
            "counting_source": self.counting_source,
            "token_triggered": self.token_triggered,
            "removed_turn_groups": self.removed_turn_groups,
            "removed_messages": self.removed_messages,
            "compaction_reason": self.compaction_reason,
        })
        .to_string();
        ContextTimelineEvent {
            kind: KIND_CONTEXT_TRIM,
            title: self.trim_title().to_string(),
            detail,
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

struct SkillHint {
    ids: Vec<String>,
    forced: bool,
}

fn skill_hint_from_messages(messages: &[Message]) -> SkillHint {
    let Some(sys) = messages.iter().find(|m| {
        m.role == "system" && !is_chat_timeline_marker(m) && m.name.is_none()
    }) else {
        return SkillHint {
            ids: Vec::new(),
            forced: false,
        };
    };
    let Some(body) = message_content_as_str(&sys.content) else {
        return SkillHint {
            ids: Vec::new(),
            forced: false,
        };
    };
    let forced = body.contains("【用户显式选用技能（/");
    let l5 = body.contains("【项目技能（skills）】");
    if !forced && !l5 {
        return SkillHint {
            ids: Vec::new(),
            forced: false,
        };
    }
    let mut ids = Vec::new();
    if let Some(id) = forced_skill_id(body) {
        push_unique(&mut ids, id);
    }
    for id in skill_file_ids(body) {
        push_unique(&mut ids, id);
    }
    SkillHint { ids, forced }
}

fn forced_skill_id(body: &str) -> Option<String> {
    const PREFIX: &str = "【用户显式选用技能（/";
    let rest = body.split_once(PREFIX)?.1;
    let id = rest.split_once('）')?.0.trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn skill_file_ids(body: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        let Some(path) = t.strip_prefix("技能文件: ") else {
            continue;
        };
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        push_unique(&mut ids, path.to_string());
    }
    ids
}

pub fn timeline_marker(event: &ContextTimelineEvent) -> Message {
    let content = serde_json::json!({
        "kind": event.kind,
        "title": event.title,
        "detail": event.detail,
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
            },
            summarized: false,
            summary_tail_kept: None,
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, KIND_CONTEXT_TRIM);
        assert_eq!(events[0].title, TITLE_TOOL_COMPRESS);
        assert!(events[0].detail.contains("\"compress_hits\":1"));
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
        assert!(events[0].detail.contains("\"before_tokens\":9000"));
        assert!(
            events[0]
                .detail
                .contains("\"compaction_reason\":\"token_budget_turn_groups\"")
        );
    }

    #[test]
    fn inject_and_trim_merge_to_at_most_two_sse() {
        let mut acc = ContextTimelineAcc::default();
        let msgs = vec![
            Message::system_only("s\n\n【项目技能（skills）】\n技能文件: foo.md\n"),
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
            },
            summarized: true,
            summary_tail_kept: Some(6),
            compaction: ContextCompactionTimelineSnapshot::default(),
        });
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].kind, KIND_CONTEXT_INJECT);
        assert_eq!(first[1].kind, KIND_CONTEXT_TRIM);
        assert_eq!(first[1].title, TITLE_TRIM);
        assert!(first[0].detail.contains("memory"));
        assert!(first[0].detail.contains("foo.md"));
        assert!(first[1].detail.contains("\"summarized\":true"));
        let second = acc.merge(ContextTimelineSnapshot {
            messages: &msgs,
            pipeline: MessagePipelineDelta {
                n_before: 8,
                n_after: 7,
                trim_count_hit: true,
                trim_char_hit: true,
                tool_compress_hits: 1,
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

    #[test]
    fn forced_skill_id_parsed() {
        let msgs = vec![Message::system_only(
            "sys\n【用户显式选用技能（/rust-build）】\n技能文件: .crabmate/skills/rust-build.md\n",
        )];
        let hint = skill_hint_from_messages(&msgs);
        assert!(hint.forced);
        assert!(hint.ids.iter().any(|s| s == "rust-build"));
    }

    #[test]
    fn skill_index_only_is_not_applied() {
        let msgs = vec![Message::system_only(
            "【项目技能索引（skills）】\n当前检测到 3 条技能\n",
        )];
        let hint = skill_hint_from_messages(&msgs);
        assert!(!hint.forced);
        assert!(hint.ids.is_empty());
    }
}
