//! 水合时保留本地 timeline / 子目标行，并按流式顺序插回服务端尾部。

use std::collections::HashSet;

use crate::storage::StoredMessage;

fn messages_contain_loading(messages: &[StoredMessage]) -> bool {
    messages
        .iter()
        .any(|m| m.state.as_ref().is_some_and(|s| s.is_loading()))
}

/// 服务端快照中不存在的本地消息：流式中的工具卡与 TimelineLog，须保留并与快照合并。
pub(crate) fn local_messages_preserved_after_hydrate(
    server_msgs: &[StoredMessage],
    local_msgs: &[StoredMessage],
) -> Vec<StoredMessage> {
    let preserve_streaming_tools = messages_contain_loading(local_msgs);
    let server_msg_ids: HashSet<_> = server_msgs.iter().map(|m| m.id.as_str()).collect();
    local_msgs
        .iter()
        .filter(|m| {
            if m.is_tool && !server_msg_ids.contains(m.id.as_str()) {
                return preserve_streaming_tools;
            }
            if let Some(ref state) = m.state {
                if state.is_local_timeline_snapshot_row() && !server_msg_ids.contains(m.id.as_str())
                {
                    return crate::timeline_scan::should_preserve_local_timeline_on_hydrate(
                        m,
                        server_msgs,
                    );
                }
                if state.looks_like_hierarchical_subgoal()
                    && !server_msg_ids.contains(m.id.as_str())
                {
                    return true;
                }
            }
            false
        })
        .cloned()
        .collect()
}

fn is_canonical_assistant_message(m: &StoredMessage) -> bool {
    m.role == "assistant"
        && !m.is_tool
        && !m
            .state
            .as_ref()
            .is_some_and(|s| s.is_local_timeline_snapshot_row())
        && !m
            .state
            .as_ref()
            .is_some_and(|s| s.looks_like_hierarchical_subgoal())
}

/// 按 local 顺序，在 merged 中找到 preserved 行应插入的位置（下一条 local 锚点之前）。
fn insertion_index_before_next_local_anchor(
    local_idx: usize,
    local_tail: &[StoredMessage],
    merged: &[StoredMessage],
) -> usize {
    for next in local_tail.iter().skip(local_idx + 1) {
        if let Some(pos) = merged.iter().position(|m| m.id == next.id) {
            return pos;
        }
        if next.role == "assistant" && !next.is_tool {
            if let Some(pos) = merged.iter().position(is_canonical_assistant_message) {
                return pos;
            }
        }
    }
    merged.len()
}

/// 将 preserved 时间线/子目标行按流式时的 local 顺序插回 merged，避免 append 到终答之后。
pub(crate) fn merge_preserved_timeline_rows_in_local_order(
    mut merged: Vec<StoredMessage>,
    preserved: &[StoredMessage],
    local_tail: &[StoredMessage],
) -> Vec<StoredMessage> {
    if preserved.is_empty() {
        return merged;
    }
    let mut sorted: Vec<&StoredMessage> = preserved.iter().collect();
    sorted.sort_by_key(|p| {
        local_tail
            .iter()
            .position(|m| m.id == p.id)
            .unwrap_or(usize::MAX)
    });
    for p in sorted {
        let Some(local_idx) = local_tail.iter().position(|m| m.id == p.id) else {
            merged.push(p.clone());
            continue;
        };
        let insert_at = insertion_index_before_next_local_anchor(local_idx, local_tail, &merged);
        merged.insert(insert_at, p.clone());
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{StoredMessage, StoredMessageState};
    use crate::timeline_scan::{timeline_state_intent_analysis_snapshot, timeline_state_tool};

    fn tool_msg(id: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "system".into(),
            text: "list_tree".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_tool(id, true)),
            is_tool: true,
            tool_call_id: None,
            tool_name: Some("list_tree".into()),
            created_at: 1,
        }
    }

    fn loading_assistant() -> StoredMessage {
        StoredMessage {
            id: "a1".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    fn user_msg(id: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "user".into(),
            text: "question".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    fn assistant_msg(id: &str, text: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "assistant".into(),
            text: text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 2,
        }
    }

    #[test]
    fn preserves_streaming_tool_rows_while_loading() {
        let server = vec![tool_msg("h_0_0")];
        let local = vec![loading_assistant(), tool_msg("sse-1")];
        let kept = local_messages_preserved_after_hydrate(&server, &local);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "sse-1");
    }

    #[test]
    fn preserves_intent_analysis_timeline_on_hydrate() {
        let server = vec![assistant_msg("a-srv", "answer")];
        let local = vec![StoredMessage {
            id: "tl-intent".into(),
            role: "assistant".into(),
            text: "意图分析：执行类\n\n".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_intent_analysis_snapshot()),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 2,
        }];
        let kept = local_messages_preserved_after_hydrate(&server, &local);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "tl-intent");
    }

    #[test]
    fn preserves_generic_local_timeline_snapshot_rows() {
        let server: Vec<StoredMessage> = vec![];
        let local = vec![StoredMessage {
            id: "tl-local".into(),
            role: "system".into(),
            text: "timeline".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::TimelineUiJson(
                r#"{"k":"cm_tl","id":"tl-local"}"#.into(),
            )),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 1,
        }];
        let kept = local_messages_preserved_after_hydrate(&server, &local);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "tl-local");
    }

    #[test]
    fn drops_local_tool_rows_after_turn_complete() {
        let server = vec![tool_msg("h_0_0")];
        let local = vec![tool_msg("sse-1"), tool_msg("h_99_1")];
        let kept = local_messages_preserved_after_hydrate(&server, &local);
        assert!(kept.is_empty());
    }

    #[test]
    fn preserves_local_hierarchical_subgoal_rows() {
        let server: Vec<StoredMessage> = vec![];
        let local = vec![StoredMessage {
            id: "sg-local".into(),
            role: "assistant".into(),
            text: "子目标执行中".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::HierarchicalSubgoal(
                "hierarchical-subgoal:goal-1".into(),
            )),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 1,
        }];
        let kept = local_messages_preserved_after_hydrate(&server, &local);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "sg-local");
    }

    #[test]
    fn intent_analysis_stays_before_canonical_answer_after_merge() {
        let intent = StoredMessage {
            id: "tl-intent".into(),
            role: "assistant".into(),
            text: "意图分析：执行类\n\n".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_intent_analysis_snapshot()),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 1,
        };
        let local_tail = vec![
            user_msg("u1"),
            intent.clone(),
            StoredMessage {
                id: "a-local".into(),
                role: "assistant".into(),
                text: String::new(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 2,
            },
        ];
        let hydrated = vec![user_msg("u1"), assistant_msg("a-srv", "final answer")];
        let preserved = vec![intent];
        let merged =
            merge_preserved_timeline_rows_in_local_order(hydrated, &preserved, &local_tail);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].id, "u1");
        assert_eq!(merged[1].id, "tl-intent");
        assert_eq!(merged[2].id, "a-srv");
    }
}
