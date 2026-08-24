//! 规范会话历史与单次 Agent 回合模型视图的分离。
//!
//! `ModelContextView` 从完整历史克隆派生；同步压缩、摘要与工具编排只修改该视图。
//! 回合成功后仅把当前真实用户消息之后新增的 assistant/tool/时间线写回规范历史。

use crate::agent::context_compaction::ContextCompactionReport;
use crate::types::{
    CRABMATE_CONTEXT_SUMMARY_NAME, CRABMATE_MODEL_CONTEXT_ARTIFACT_NAME, Message, MessageContent,
    user_message_counts_for_branch_truncation,
};

pub const MODEL_CONTEXT_ARTIFACT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CanonicalMessageRange {
    pub start: usize,
    pub end: usize,
}

/// 可持久化、可回放的派生视图配方。被移出的原消息仍保留在规范历史中。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ModelContextArtifact {
    pub schema_version: u32,
    pub model_call_sequence: usize,
    pub canonical_message_count_before_turn: usize,
    pub model_view_message_count: usize,
    pub compaction: ContextCompactionReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_turn_group_ranges: Vec<CanonicalMessageRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summarized_canonical_range: Option<CanonicalMessageRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_tail_kept: Option<usize>,
    /// 该模型调用看到的当前真实用户交互组（含已压缩工具结果），用于重放多轮工具链。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current_turn_messages: Vec<Message>,
}

impl ModelContextArtifact {
    #[must_use]
    pub fn capture(
        model_call_sequence: usize,
        canonical_message_count_before_turn: usize,
        model_messages: &[Message],
        compaction: ContextCompactionReport,
        summary_tail_kept: Option<usize>,
    ) -> Self {
        let summary_text = model_messages.iter().rev().find_map(|message| {
            (message.role == "user"
                && message.name.as_deref() == Some(CRABMATE_CONTEXT_SUMMARY_NAME))
            .then(|| match &message.content {
                Some(MessageContent::Text(text)) => Some(text.clone()),
                _ => None,
            })
            .flatten()
        });
        let current_turn_messages = model_messages
            .iter()
            .rposition(user_message_counts_for_branch_truncation)
            .map(|start| {
                model_messages[start..]
                    .iter()
                    .filter(|message| {
                        !crate::types::is_server_injected_user_message(message)
                            && !crate::types::is_execution_constraint_ephemeral_system(message)
                            && !crate::types::is_message_excluded_from_llm_context_except_memory(
                                message,
                            )
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Self {
            schema_version: MODEL_CONTEXT_ARTIFACT_SCHEMA_VERSION,
            model_call_sequence,
            canonical_message_count_before_turn,
            model_view_message_count: model_messages.len(),
            compaction,
            summary_text,
            removed_turn_group_ranges: Vec::new(),
            summarized_canonical_range: None,
            summary_tail_kept,
            current_turn_messages,
        }
    }

    pub fn bind_canonical_ranges(&mut self, canonical: &[Message]) {
        let prefix_len = self
            .canonical_message_count_before_turn
            .min(canonical.len());
        let prefix = &canonical[..prefix_len];
        self.removed_turn_group_ranges =
            crate::cm_agent::message_pipeline::conversation_turn_groups(prefix)
                .into_iter()
                .take(self.compaction.removed_turn_groups)
                .map(|group| CanonicalMessageRange {
                    start: group.start,
                    end: group.end,
                })
                .collect();
        self.summarized_canonical_range = self.summary_tail_kept.and_then(|tail| {
            let start = usize::from(prefix.first().is_some_and(|message| message.role == "system"));
            let end = prefix_len.saturating_sub(tail);
            (end > start).then_some(CanonicalMessageRange { start, end })
        });
    }

    #[must_use]
    pub fn replay_removed_messages(&self, canonical: &[Message]) -> Vec<Message> {
        let mut ranges = self.removed_turn_group_ranges.clone();
        if let Some(range) = self.summarized_canonical_range {
            ranges.push(range);
        }
        ranges.sort_by_key(|range| (range.start, range.end));
        let mut out = Vec::new();
        let mut last_end = 0;
        for range in ranges {
            let start = range.start.max(last_end).min(canonical.len());
            let end = range.end.min(canonical.len());
            if start < end {
                out.extend_from_slice(&canonical[start..end]);
                last_end = end;
            }
        }
        out
    }

    #[must_use]
    pub fn into_marker(self) -> Option<Message> {
        let content = serde_json::to_string(&self).ok()?;
        Some(Message {
            role: "system".to_string(),
            content: Some(MessageContent::Text(content)),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(CRABMATE_MODEL_CONTEXT_ARTIFACT_NAME.to_string()),
            tool_call_id: None,
        })
    }
}

#[must_use]
pub fn artifacts_from_messages(messages: &[Message]) -> Vec<ModelContextArtifact> {
    messages
        .iter()
        .filter_map(|message| {
            crate::types::is_model_context_artifact_marker(message)
                .then(|| crate::types::message_content_as_str(&message.content))
                .flatten()
                .and_then(|raw| serde_json::from_str(raw).ok())
        })
        .collect()
}

/// 单轮派生模型视图；持有规范历史克隆，不共享可变底层。
#[derive(Clone, Debug)]
pub struct ModelContextView {
    messages: Vec<Message>,
    canonical_message_count_before_turn: usize,
    original_parked_sidecars: Vec<Message>,
}

impl ModelContextView {
    #[must_use]
    pub fn derive(canonical: &[Message]) -> Self {
        Self {
            messages: canonical.to_vec(),
            canonical_message_count_before_turn: canonical.len(),
            original_parked_sidecars: canonical
                .iter()
                .filter(|message| {
                    crate::types::is_chat_timeline_marker(message)
                        || crate::types::is_model_context_artifact_marker(message)
                })
                .cloned()
                .collect(),
        }
    }

    pub fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.messages
    }

    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    #[must_use]
    pub const fn canonical_message_count_before_turn(&self) -> usize {
        self.canonical_message_count_before_turn
    }

    /// 将派生视图中当前真实用户回合新增的消息追加回规范历史。
    ///
    /// 历史压缩、摘要替换和删除只发生在视图副本，不会覆盖规范历史。
    pub fn commit_current_turn_to(&self, canonical: &mut Vec<Message>) -> usize {
        let Some(model_anchor) = self
            .messages
            .iter()
            .rposition(user_message_counts_for_branch_truncation)
        else {
            return 0;
        };
        let additions = &self.messages[model_anchor.saturating_add(1)..];
        if additions
            .iter()
            .any(crate::cm_agent::context_timeline::is_context_window_timeline_marker)
        {
            crate::cm_agent::context_timeline::strip_context_window_timeline_markers(canonical);
        }
        let additions: Vec<Message> = additions
            .iter()
            .filter(|message| {
                crate::cm_agent::context_timeline::is_context_window_timeline_marker(message)
                    || !self
                        .original_parked_sidecars
                        .iter()
                        .any(|original| original == *message)
            })
            .cloned()
            .collect();
        let added = additions.len();
        canonical.extend(additions);
        added
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_compaction_does_not_delete_canonical_history() {
        let mut canonical = vec![
            Message::system_only("system"),
            Message::user_only("old user"),
            Message::assistant_only("old answer"),
            Message::user_only("current user"),
        ];
        let original = canonical.clone();
        let mut view = ModelContextView::derive(&canonical);
        view.messages_mut().drain(1..3);
        view.messages_mut()
            .push(Message::assistant_only("current answer"));

        assert_eq!(canonical, original);
        assert_eq!(view.commit_current_turn_to(&mut canonical), 1);
        assert_eq!(canonical.len(), original.len() + 1);
        assert!(canonical.iter().any(|message| {
            crate::types::message_content_as_str(&message.content) == Some("old answer")
        }));
    }

    #[test]
    fn artifact_roundtrips_as_hidden_system_marker() {
        let artifact = ModelContextArtifact::capture(
            1,
            4,
            &[Message::user_context_summary_injection("summary")],
            ContextCompactionReport::default(),
            Some(2),
        );
        let marker = artifact.clone().into_marker().expect("serialize artifact");
        assert_eq!(
            marker.name.as_deref(),
            Some(CRABMATE_MODEL_CONTEXT_ARTIFACT_NAME)
        );
        let raw = crate::types::message_content_as_str(&marker.content).expect("marker text");
        let decoded: ModelContextArtifact = serde_json::from_str(raw).expect("decode artifact");
        assert_eq!(decoded, artifact);
    }

    #[test]
    fn artifact_replays_removed_canonical_turn_group() {
        let canonical = vec![
            Message::system_only("system"),
            Message::user_only("old user"),
            Message::assistant_only("old answer"),
            Message::user_only("current user"),
        ];
        let report = ContextCompactionReport {
            removed_turn_groups: 1,
            removed_messages: 2,
            ..Default::default()
        };
        let mut artifact =
            ModelContextArtifact::capture(1, canonical.len(), &canonical[3..], report, None);
        artifact.bind_canonical_ranges(&canonical);
        let replay = artifact.replay_removed_messages(&canonical);
        assert_eq!(replay, canonical[1..3]);
    }

    #[test]
    fn artifact_current_turn_omits_ephemeral_server_injections() {
        let mut memory = Message::user_only("private memory");
        memory.name = Some(crate::types::CRABMATE_LONG_TERM_MEMORY_NAME.to_string());
        let messages = vec![
            Message::user_only("current"),
            memory,
            Message::assistant_only("answer"),
        ];
        let artifact =
            ModelContextArtifact::capture(1, 1, &messages, ContextCompactionReport::default(), None);
        assert_eq!(artifact.current_turn_messages.len(), 2);
        assert!(
            artifact
                .current_turn_messages
                .iter()
                .all(|message| !crate::types::is_server_injected_user_message(message))
        );
    }

    #[test]
    fn commit_does_not_duplicate_parked_sidecars_from_old_turns() {
        let old_timeline = Message {
            role: "system".to_string(),
            content: Some(MessageContent::Text(
                r#"{"kind":"context_trim","title":"old"}"#.to_string(),
            )),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("crabmate_timeline".to_string()),
            tool_call_id: None,
        };
        let mut canonical = vec![
            Message::system_only("system"),
            Message::user_only("old"),
            old_timeline.clone(),
            Message::assistant_only("old answer"),
            Message::user_only("current"),
        ];
        let mut view = ModelContextView::derive(&canonical);
        view.messages_mut().retain(|message| message != &old_timeline);
        view.messages_mut().push(Message::assistant_only("new answer"));
        view.messages_mut().push(old_timeline.clone());

        assert_eq!(view.commit_current_turn_to(&mut canonical), 2);
        assert_eq!(
            canonical
                .iter()
                .filter(|message| *message == &old_timeline)
                .count(),
            1
        );
    }
}
