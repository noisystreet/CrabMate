//! 会话尾部水合：**唯一**合并入口 [`merge_session_tail`]。
//!
//! ## 算法（维护时只改本模块）
//!
//! 1. **服务端快照净化**：去掉展示层隐藏的注入类 `user`。
//! 2. **本地 plain user 保留**：服务端缺失或误含注入 user 时，按 local 补回真实 user。
//! 3. **按 local 顺序回放**：已落盘行取服务端内容；timeline / 子目标 / 流式工具等 local 独占行原位保留；
//!    canonical assistant / tool 由 id 或池匹配；`loading` 占位跳过；`placed_ids` 保证幂等。

use std::collections::{HashMap, HashSet, VecDeque};

use crate::storage::StoredMessage;

fn messages_contain_loading(messages: &[StoredMessage]) -> bool {
    messages
        .iter()
        .any(|m| m.state.as_ref().is_some_and(|s| s.is_loading()))
}

fn is_plain_user_bubble(m: &StoredMessage) -> bool {
    m.role == "user"
        && !m.is_tool
        && !crabmate_display_rules::user_message_should_hide_for_chat_display(m.text.as_str())
}

fn local_plain_user_bubbles_preserved(
    server_msgs: &[StoredMessage],
    local_msgs: &[StoredMessage],
) -> Vec<StoredMessage> {
    local_msgs
        .iter()
        .filter(|m| {
            if !is_plain_user_bubble(m) {
                return false;
            }
            let t = m.text.trim();
            if t.is_empty() {
                return false;
            }
            !server_msgs
                .iter()
                .any(|s| s.role == "user" && s.text.trim() == t)
        })
        .cloned()
        .collect()
}

fn inject_preserved_plain_users(
    mut server: Vec<StoredMessage>,
    local_tail: &[StoredMessage],
) -> Vec<StoredMessage> {
    let preserved = local_plain_user_bubbles_preserved(&server, local_tail);
    if preserved.is_empty() {
        return server;
    }
    server.retain(|m| {
        !(m.role == "user"
            && crabmate_display_rules::user_message_should_hide_for_chat_display(m.text.as_str()))
    });
    if let Some(pos) = server.iter().position(|m| m.role == "user") {
        for (i, u) in preserved.iter().enumerate() {
            server.insert(pos + i, u.clone());
        }
    } else {
        let mut prefixed = preserved;
        prefixed.append(&mut server);
        server = prefixed;
    }
    server
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

fn push_once(out: &mut Vec<StoredMessage>, placed_ids: &mut HashSet<String>, msg: StoredMessage) {
    if placed_ids.insert(msg.id.clone()) {
        out.push(msg);
    }
}

fn matching_server_user(
    server: &[StoredMessage],
    placed_ids: &HashSet<String>,
    local: &StoredMessage,
) -> Option<StoredMessage> {
    server
        .iter()
        .find(|h| {
            h.role == "user" && !placed_ids.contains(&h.id) && h.text.trim() == local.text.trim()
        })
        .cloned()
}

fn hydrated_or_matching_user(
    hydrated_by_id: &HashMap<String, StoredMessage>,
    server: &[StoredMessage],
    placed_ids: &HashSet<String>,
    local: &StoredMessage,
) -> StoredMessage {
    hydrated_by_id
        .get(&local.id)
        .cloned()
        .or_else(|| matching_server_user(server, placed_ids, local))
        .unwrap_or_else(|| local.clone())
}

fn push_next_from_pool(
    out: &mut Vec<StoredMessage>,
    placed_ids: &mut HashSet<String>,
    pool: &mut VecDeque<StoredMessage>,
) {
    if let Some(h) = pop_next_unplaced(pool, placed_ids) {
        push_once(out, placed_ids, h);
    }
}

fn replay_local_order_against_server(
    server: Vec<StoredMessage>,
    local_tail: &[StoredMessage],
) -> Vec<StoredMessage> {
    let hydrated_by_id: HashMap<_, _> = server.iter().map(|m| (m.id.clone(), m.clone())).collect();
    let mut assistant_pool: VecDeque<_> = server
        .iter()
        .filter(|m| is_canonical_assistant_message(m))
        .cloned()
        .collect();
    let mut tool_pool: VecDeque<_> = server.iter().filter(|m| m.is_tool).cloned().collect();

    let mut out = Vec::with_capacity(local_tail.len().max(server.len()));
    let mut placed_ids = HashSet::new();

    for local in local_tail {
        if is_local_only_row_to_replay(local, &server, local_tail) {
            push_once(&mut out, &mut placed_ids, local.clone());
            continue;
        }
        if local.role == "user" && !local.is_tool {
            let h = hydrated_or_matching_user(&hydrated_by_id, &server, &placed_ids, local);
            push_once(&mut out, &mut placed_ids, h);
            continue;
        }
        if let Some(h) = hydrated_by_id.get(&local.id) {
            push_once(&mut out, &mut placed_ids, h.clone());
            continue;
        }
        if local.state.as_ref().is_some_and(|s| s.is_loading()) {
            continue;
        }
        if local.role == "assistant" && !local.is_tool {
            push_next_from_pool(&mut out, &mut placed_ids, &mut assistant_pool);
            continue;
        }
        if local.is_tool {
            push_next_from_pool(&mut out, &mut placed_ids, &mut tool_pool);
        }
    }

    for h in server {
        push_once(&mut out, &mut placed_ids, h);
    }
    out
}

/// 将服务端尾部快照与本地 `local_tail` 合并为**单一**消息序列（水合写回 `ChatSession.messages` 的唯一路径）。
#[must_use]
pub(crate) fn merge_session_tail(
    server_hydrated: Vec<StoredMessage>,
    local_tail: &[StoredMessage],
) -> Vec<StoredMessage> {
    let server = inject_preserved_plain_users(server_hydrated, local_tail);
    replay_local_order_against_server(server, local_tail)
}

#[cfg(test)]
mod golden {
    use super::*;
    use crate::storage::{StoredMessage, StoredMessageState};
    use crate::timeline_scan::{timeline_state_intent_analysis_snapshot, timeline_state_tool};

    fn user_msg(id: &str, text: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "user".into(),
            text: text.into(),
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

    fn intent_msg(id: &str, text: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "assistant".into(),
            text: text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_intent_analysis_snapshot()),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 1,
        }
    }

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

    fn roles(msgs: &[StoredMessage]) -> Vec<&str> {
        msgs.iter().map(|m| m.role.as_str()).collect()
    }

    #[test]
    fn golden_restores_plain_user_over_injection() {
        const REAL: &str = "用户真实诉求";
        let reject = format!(
            "{} 请仅输出 JSON",
            crabmate_display_rules::STAGED_PLANNER_TOOL_CALL_REJECT_PREFIX
        );
        let server = vec![user_msg("srv-inj", &reject), assistant_msg("a1", "ok")];
        let local = vec![user_msg("local-u", REAL)];
        let merged = merge_session_tail(server, &local);
        assert!(
            merged
                .iter()
                .any(|m| m.role == "user" && m.text.contains(REAL))
        );
        assert!(!merged.iter().any(|m| {
            crabmate_display_rules::is_planner_tool_call_reject_injected_user_content(
                m.text.as_str(),
            )
        }));
    }

    #[test]
    fn golden_user_before_assistants_when_server_omits_user() {
        let local = vec![user_msg("local-u", "你好")];
        let server = vec![
            assistant_msg("intent", "意图分析：问候类\n\n"),
            assistant_msg("a1", "你好！"),
        ];
        let merged = merge_session_tail(server, &local);
        assert_eq!(roles(&merged), vec!["user", "assistant", "assistant"]);
        assert_eq!(merged[0].text, "你好");
    }

    #[test]
    fn golden_greeting_turn_user_intent_answer() {
        let local = vec![
            user_msg("u1", "你好"),
            intent_msg("tl-intent", "意图分析：问候类\n\n"),
            assistant_msg("a-local", "你好！"),
        ];
        let server = vec![user_msg("u1", "你好"), assistant_msg("a-srv", "你好！")];
        let merged = merge_session_tail(server, &local);
        assert_eq!(roles(&merged), vec!["user", "assistant", "assistant"]);
        assert_eq!(merged[0].text, "你好");
        assert!(merged[1].text.contains("意图分析"));
    }

    #[test]
    fn golden_server_omits_user_keeps_local_before_intent() {
        let local = vec![
            user_msg("u-local", "你好"),
            intent_msg("tl-intent", "意图分析：问候类\n\n"),
            assistant_msg("a-local", "你好！"),
        ];
        let server = vec![
            intent_msg("tl-intent-srv", "意图分析：问候类\n\n"),
            assistant_msg("a-srv", "你好！我是 CrabMate 的 AI 助手。"),
        ];
        let merged = merge_session_tail(server, &local);
        assert_eq!(roles(&merged), vec!["user", "assistant", "assistant"]);
        assert_eq!(merged[0].text, "你好");
    }

    #[test]
    fn golden_intent_before_answer_when_local_has_timeline() {
        let local = vec![
            user_msg("u1", "question"),
            intent_msg("tl-intent", "意图分析：执行类\n\n"),
            assistant_msg("a-local", "stream draft"),
        ];
        let server = vec![
            user_msg("u1", "question"),
            assistant_msg("a-srv", "final answer"),
        ];
        let merged = merge_session_tail(server, &local);
        let ids: Vec<_> = merged.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "tl-intent", "a-srv"]);
    }

    #[test]
    fn golden_two_turn_skills_idempotent() {
        let intent_text = "意图分析：问答类（直接回复）\n\n";
        let local = vec![
            user_msg("u1", "你好"),
            assistant_msg("a1", "你好！"),
            user_msg("u2", "你有哪些技能"),
            intent_msg("local-intent", intent_text),
            assistant_msg("a2-local", ""),
        ];
        let server = vec![
            user_msg("u1", "你好"),
            assistant_msg("a1", "你好！"),
            user_msg("u2", "你有哪些技能"),
            assistant_msg("srv-intent", intent_text),
            assistant_msg("a2-srv", "技能列表…"),
        ];
        let once = merge_session_tail(server.clone(), &local);
        let twice = merge_session_tail(once.clone(), &local);
        assert_eq!(
            once.len(),
            5,
            "{:?}",
            once.iter().map(|m| &m.id).collect::<Vec<_>>()
        );
        assert_eq!(once.len(), twice.len());
        assert_eq!(
            once.iter().filter(|m| m.text.contains("意图分析")).count(),
            1
        );
        assert_eq!(
            roles(&once),
            vec!["user", "assistant", "user", "assistant", "assistant"]
        );
    }

    #[test]
    fn golden_skips_loading_and_replays_server_answer() {
        let local = vec![
            user_msg("u1", "question"),
            intent_msg("tl-intent", "意图分析：执行类\n\n"),
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
        let server = vec![
            user_msg("u1", "question"),
            assistant_msg("a-srv", "final answer"),
        ];
        let merged = merge_session_tail(server, &local);
        let ids: Vec<_> = merged.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "tl-intent", "a-srv"]);
    }

    #[test]
    fn golden_tool_pool_by_local_order() {
        let local = vec![
            user_msg("u1", "question"),
            tool_msg("sse-tool"),
            assistant_msg("a-local", "draft"),
        ];
        let server = vec![
            user_msg("u1", "question"),
            tool_msg("h_0_0"),
            assistant_msg("a-srv", "final answer"),
        ];
        let merged = merge_session_tail(server, &local);
        let ids: Vec<_> = merged.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "h_0_0", "a-srv"]);
    }

    #[test]
    fn golden_drops_ephemeral_sse_tools_after_turn_complete() {
        let local = vec![tool_msg("sse-1"), tool_msg("h_99_1")];
        let server = vec![tool_msg("h_0_0")];
        let merged = merge_session_tail(server, &local);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "h_0_0");
    }

    #[test]
    fn golden_hierarchical_subgoal_between_tool_and_answer() {
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
        let local = vec![
            user_msg("u1", "question"),
            tool_msg("sse-tool"),
            subgoal,
            assistant_msg("a-local", "draft"),
        ];
        let server = vec![
            user_msg("u1", "question"),
            tool_msg("h_0_0"),
            assistant_msg("a-srv", "final answer"),
        ];
        let merged = merge_session_tail(server, &local);
        let ids: Vec<_> = merged.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "h_0_0", "sg-local", "a-srv"]);
    }

    #[test]
    fn golden_keeps_sse_tool_while_loading() {
        let local = vec![
            StoredMessage {
                id: "a-loading".into(),
                role: "assistant".into(),
                text: String::new(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            tool_msg("sse-1"),
        ];
        let server = vec![tool_msg("h_0_0")];
        let merged = merge_session_tail(server, &local);
        assert!(merged.iter().any(|m| m.id == "sse-1"));
    }
}
