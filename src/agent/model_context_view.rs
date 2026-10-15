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

    /// 将派生视图的结构差异提交回规范历史（reconcile，见设计文档 §3.7）。
    ///
    /// 只提交**结构**变化：被移出的连续原文前缀（交由调用方归档）、可选插入的基线摘要、
    /// 以及当前真实用户回合之后新增的消息。窗口内其后原文逐字保留；就地内容改写不回写。
    pub fn commit_current_turn_to(&self, canonical: &mut Vec<Message>) -> StructuralCommit {
        let Some(view_anchor) = self
            .messages
            .iter()
            .rposition(user_message_counts_for_branch_truncation)
        else {
            return StructuralCommit::default();
        };
        let view_head = &self.messages[..=view_anchor];
        let additions: Vec<Message> = self.messages[view_anchor.saturating_add(1)..]
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

        let removed_users =
            count_real_users(canonical).saturating_sub(count_real_users(view_head));
        let view_summary = last_summary_message(&self.messages).cloned();
        let summary_replaced = match (view_summary.as_ref(), last_summary_message(canonical)) {
            (Some(view), Some(existing)) => view != existing,
            (Some(_), None) => true,
            _ => false,
        };
        let first_removal = if summary_replaced {
            usize::from(
                canonical
                    .first()
                    .is_some_and(|message| message.role == "system"),
            )
        } else {
            nth_real_user_index(canonical, 0).unwrap_or(canonical.len())
        };
        let tail_start = nth_real_user_index(canonical, removed_users)
            .unwrap_or(canonical.len())
            .max(first_removal);
        let archived_prefix = canonical[first_removal..tail_start].to_vec();

        let mut next = canonical[..first_removal].to_vec();
        if summary_replaced && let Some(summary) = view_summary {
            next.push(summary);
        }
        next.extend_from_slice(&canonical[tail_start..]);
        if additions
            .iter()
            .any(crate::cm_agent::context_timeline::is_context_window_timeline_marker)
        {
            crate::cm_agent::context_timeline::strip_context_window_timeline_markers(&mut next);
        }
        let appended = additions.len();
        next.extend(additions);
        let archived_artifacts = enforce_single_artifact(&mut next);
        *canonical = next;

        StructuralCommit {
            archived_prefix,
            prefix_summarized: summary_replaced,
            archived_artifacts,
            appended,
            range_start: first_removal,
            range_end: tail_start,
        }
    }
}

/// 结构性提交结果：本轮被移出 canonical 的原文（归档候选）与追加条数（见 §3.7）。
#[derive(Clone, Debug, Default)]
pub struct StructuralCommit {
    /// 被移出 canonical 的连续原文前缀（逐字原文，交由调用方归档）。
    pub archived_prefix: Vec<Message>,
    /// 该前缀是否由摘要替换移出（`true` → `summarized`，否则 `removed_turn_group`）。
    pub prefix_summarized: bool,
    /// 超出「canonical 至多 1 条」上限、被回收的旧 artifact 旁注（见 §3.9）。
    pub archived_artifacts: Vec<Message>,
    /// 本次追加回 canonical 的当前回合消息条数。
    pub appended: usize,
    /// 移出区间在提交前 canonical 中的下标快照（**仅**人审 / 排查）。
    pub range_start: usize,
    pub range_end: usize,
}

impl StructuralCommit {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.archived_prefix.is_empty() && self.archived_artifacts.is_empty() && self.appended == 0
    }
}

fn count_real_users(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|message| user_message_counts_for_branch_truncation(message))
        .count()
}

fn nth_real_user_index(messages: &[Message], n: usize) -> Option<usize> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, message)| user_message_counts_for_branch_truncation(message))
        .nth(n)
        .map(|(index, _)| index)
}

fn last_summary_message(messages: &[Message]) -> Option<&Message> {
    messages.iter().rev().find(|message| {
        message.role == "user" && message.name.as_deref() == Some(CRABMATE_CONTEXT_SUMMARY_NAME)
    })
}

/// 回收多余的 artifact 旁注：只保留最后 1 条，其余按序返回（见 §3.9）。
fn enforce_single_artifact(messages: &mut Vec<Message>) -> Vec<Message> {
    let indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| crate::types::is_model_context_artifact_marker(message))
        .map(|(index, _)| index)
        .collect();
    let Some((&keep, recycled)) = indices.split_last() else {
        return Vec::new();
    };
    let archived = recycled
        .iter()
        .map(|&index| messages[index].clone())
        .collect();
    let mut cursor = 0usize;
    messages.retain(|_| {
        let index = cursor;
        cursor += 1;
        index == keep || !recycled.contains(&index)
    });
    archived
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_compaction_archives_removed_prefix_and_keeps_canonical_tail() {
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
        let commit = view.commit_current_turn_to(&mut canonical);
        assert_eq!(commit.appended, 1);
        assert!(!commit.prefix_summarized);
        assert_eq!(commit.archived_prefix, original[1..3].to_vec());
        assert_eq!(commit.range_start, 1);
        assert_eq!(commit.range_end, 3);
        // 被移出的原文前缀内容仍在归档候选里，可从 canonical 之外无损取回。
        assert!(commit.archived_prefix.iter().any(|message| {
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

        assert_eq!(view.commit_current_turn_to(&mut canonical).appended, 2);
        assert_eq!(
            canonical
                .iter()
                .filter(|message| *message == &old_timeline)
                .count(),
            1
        );
    }

    #[test]
    fn commit_replaces_prefix_with_summary_and_archives_original_prefix() {
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
            .insert(1, Message::user_context_summary_injection("summary"));
        view.messages_mut()
            .push(Message::assistant_only("current answer"));

        let commit = view.commit_current_turn_to(&mut canonical);
        assert!(commit.prefix_summarized);
        assert_eq!(commit.appended, 1);
        assert_eq!(commit.archived_prefix, original[1..3].to_vec());
        assert_eq!(canonical.len(), 4);
        assert_eq!(canonical[0], original[0]);
        assert_eq!(
            canonical[1].name.as_deref(),
            Some(CRABMATE_CONTEXT_SUMMARY_NAME)
        );
        assert_eq!(canonical[2], original[3]);
    }

    #[test]
    fn commit_recycles_old_context_artifacts_beyond_single_canonical_marker() {
        let artifact_a = ModelContextArtifact::capture(
            1,
            0,
            &[],
            ContextCompactionReport::default(),
            None,
        )
        .into_marker()
        .expect("marker a");
        let artifact_b = ModelContextArtifact::capture(
            2,
            0,
            &[],
            ContextCompactionReport::default(),
            None,
        )
        .into_marker()
        .expect("marker b");
        let mut canonical = vec![
            Message::system_only("system"),
            artifact_a.clone(),
            artifact_b.clone(),
            Message::user_only("current user"),
        ];
        let mut view = ModelContextView::derive(&canonical);
        view.messages_mut()
            .push(Message::assistant_only("current answer"));

        let commit = view.commit_current_turn_to(&mut canonical);
        assert_eq!(commit.archived_artifacts, vec![artifact_a]);
        assert_eq!(
            canonical
                .iter()
                .filter(|message| crate::types::is_model_context_artifact_marker(message))
                .count(),
            1
        );
    }
}
