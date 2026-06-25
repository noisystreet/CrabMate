//! 尾部水合：以 **local 消息顺序** 为真源回放，服务端快照提供已落盘行的内容。

use std::collections::{HashMap, HashSet, VecDeque};

use crate::storage::StoredMessage;

fn messages_contain_loading(messages: &[StoredMessage]) -> bool {
    messages
        .iter()
        .any(|m| m.state.as_ref().is_some_and(|s| s.is_loading()))
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

/// 本地独占行：服务端无同 id，且仍应保留在回放结果中（timeline、子目标、流式工具等）。
fn is_local_only_row_to_replay(
    m: &StoredMessage,
    server_msgs: &[StoredMessage],
    local_msgs: &[StoredMessage],
) -> bool {
    let preserve_streaming_tools = messages_contain_loading(local_msgs);
    let server_msg_ids: HashSet<_> = server_msgs.iter().map(|x| x.id.as_str()).collect();
    if m.is_tool && !server_msg_ids.contains(m.id.as_str()) {
        return preserve_streaming_tools;
    }
    if let Some(ref state) = m.state {
        if state.is_local_timeline_snapshot_row() && !server_msg_ids.contains(m.id.as_str()) {
            return crate::timeline_scan::should_preserve_local_timeline_on_hydrate(m, server_msgs);
        }
        if state.looks_like_hierarchical_subgoal() && !server_msg_ids.contains(m.id.as_str()) {
            return true;
        }
    }
    false
}

fn pop_next_unplaced(
    queue: &mut VecDeque<StoredMessage>,
    placed_ids: &HashSet<String>,
) -> Option<StoredMessage> {
    while let Some(h) = queue.pop_front() {
        if !placed_ids.contains(&h.id) {
            return Some(h);
        }
    }
    None
}

fn push_hydrated_once(
    out: &mut Vec<StoredMessage>,
    placed_ids: &mut HashSet<String>,
    msg: StoredMessage,
) {
    if placed_ids.insert(msg.id.clone()) {
        out.push(msg);
    }
}

/// 按 `local_tail` 顺序回放尾部：已落盘行取 `hydrated` 内容，未落盘 timeline/子目标保留本地副本。
pub(crate) fn merge_hydrated_tail_replaying_local_order(
    hydrated: Vec<StoredMessage>,
    local_tail: &[StoredMessage],
) -> Vec<StoredMessage> {
    let hydrated_by_id: HashMap<_, _> =
        hydrated.iter().map(|m| (m.id.clone(), m.clone())).collect();
    let mut assistant_pool: VecDeque<_> = hydrated
        .iter()
        .filter(|m| is_canonical_assistant_message(m))
        .cloned()
        .collect();
    let mut tool_pool: VecDeque<_> = hydrated.iter().filter(|m| m.is_tool).cloned().collect();

    let mut out = Vec::with_capacity(local_tail.len().max(hydrated.len()));
    let mut placed_ids = HashSet::new();

    for local in local_tail {
        if is_local_only_row_to_replay(local, &hydrated, local_tail) {
            out.push(local.clone());
            continue;
        }
        if let Some(h) = hydrated_by_id.get(&local.id) {
            push_hydrated_once(&mut out, &mut placed_ids, h.clone());
            continue;
        }
        if local.state.as_ref().is_some_and(|s| s.is_loading()) {
            continue;
        }
        if local.role == "assistant" && !local.is_tool {
            if let Some(h) = pop_next_unplaced(&mut assistant_pool, &placed_ids) {
                push_hydrated_once(&mut out, &mut placed_ids, h);
            }
            continue;
        }
        if local.is_tool {
            if let Some(h) = pop_next_unplaced(&mut tool_pool, &placed_ids) {
                push_hydrated_once(&mut out, &mut placed_ids, h);
            }
        }
    }

    for h in hydrated {
        push_hydrated_once(&mut out, &mut placed_ids, h);
    }
    out
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
    fn local_only_streaming_tool_while_loading() {
        let server = vec![tool_msg("h_0_0")];
        let local = vec![loading_assistant(), tool_msg("sse-1")];
        assert!(is_local_only_row_to_replay(&local[1], &server, &local));
    }

    #[test]
    fn intent_analysis_is_local_only_and_replays_before_answer() {
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
        assert!(is_local_only_row_to_replay(&intent, &hydrated, &local_tail));
        let merged = merge_hydrated_tail_replaying_local_order(hydrated, &local_tail);
        let ids: Vec<_> = merged.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "tl-intent", "a-srv"]);
    }

    #[test]
    fn replays_tools_from_hydrated_pool_when_local_sse_ids_differ() {
        let local_tail = vec![
            user_msg("u1"),
            tool_msg("sse-tool"),
            assistant_msg("a-local", "draft"),
        ];
        let hydrated = vec![
            user_msg("u1"),
            tool_msg("h_0_0"),
            assistant_msg("a-srv", "final answer"),
        ];
        let merged = merge_hydrated_tail_replaying_local_order(hydrated, &local_tail);
        let ids: Vec<_> = merged.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "h_0_0", "a-srv"]);
    }

    #[test]
    fn hierarchical_subgoal_stays_between_tool_and_answer() {
        let subgoal = StoredMessage {
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
            created_at: 2,
        };
        let local_tail = vec![
            user_msg("u1"),
            tool_msg("sse-tool"),
            subgoal.clone(),
            assistant_msg("a-local", "draft"),
        ];
        let hydrated = vec![
            user_msg("u1"),
            tool_msg("h_0_0"),
            assistant_msg("a-srv", "final answer"),
        ];
        let merged = merge_hydrated_tail_replaying_local_order(hydrated, &local_tail);
        let ids: Vec<_> = merged.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "h_0_0", "sg-local", "a-srv"]);
    }

    #[test]
    fn drops_ephemeral_sse_tools_after_turn_complete() {
        let server = vec![tool_msg("h_0_0")];
        let local = vec![tool_msg("sse-1"), tool_msg("h_99_1")];
        assert!(!is_local_only_row_to_replay(&local[0], &server, &local));
        let merged = merge_hydrated_tail_replaying_local_order(server.clone(), &local);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "h_0_0");
    }
}
