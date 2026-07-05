//! 单一读路径：聊天列与 Markdown/JSON 导出共用的可见消息筛选。
//!
//! **写**经 `TurnReducer` + `sync_turn_projection`（见 `docs/Turn布局设计.md` §12.7）；
//! **读**统一经本模块做 scope 过滤；Phase 7 P1 起 **不再** 对 assistant 正文做 fuzzy dedupe（去重由写入收敛保证）。

use crate::message_dedupe::assistant_texts_fuzzy_duplicate;
use crate::storage::StoredMessage;
use crate::timeline_scan::{
    is_commentary_before_tools_assistant, is_ephemeral_timeline_assistant_for_export,
    is_orchestration_route_timeline_message, timeline_ui_snapshot_type,
};

/// 可见消息筛选范围（聊天列保留 loading 尾泡；导出去掉 ephemeral 与空 loading 壳）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleMessageScope {
    ChatColumn,
    Export,
}

fn is_loading_assistant_shell(m: &StoredMessage) -> bool {
    m.role == "assistant" && !m.is_tool && m.state.as_ref().is_some_and(|st| st.is_loading())
}

fn is_duplicate_final_response_snapshot(
    m: &StoredMessage,
    messages: &[StoredMessage],
    idx: usize,
) -> bool {
    if m.state
        .as_ref()
        .and_then(timeline_ui_snapshot_type)
        .as_deref()
        != Some("final_response_snapshot")
    {
        return false;
    }
    messages[..idx].iter().any(|other| {
        other.role == "assistant"
            && !other.is_tool
            && other
                .state
                .as_ref()
                .and_then(timeline_ui_snapshot_type)
                .as_deref()
                != Some("final_response_snapshot")
            && assistant_texts_fuzzy_duplicate(other.text.as_str(), m.text.as_str())
    })
}

/// 该条是否应从用户可见列表中隐藏。
pub fn is_message_hidden_from_view(
    m: &StoredMessage,
    messages: &[StoredMessage],
    idx: usize,
    scope: VisibleMessageScope,
) -> bool {
    match scope {
        VisibleMessageScope::ChatColumn => {
            if is_orchestration_route_timeline_message(m) {
                return true;
            }
            if is_commentary_before_tools_assistant(m) {
                return true;
            }
            if is_duplicate_final_response_snapshot(m, messages, idx) {
                return true;
            }
            // 隐藏空的 loading 壳：首条 delta 到达后文本非空，气泡自然出现。
            if is_loading_assistant_shell(m)
                && m.text.trim().is_empty()
                && m.reasoning_text.trim().is_empty()
            {
                return true;
            }
            false
        }
        VisibleMessageScope::Export => {
            if is_ephemeral_timeline_assistant_for_export(m, messages) {
                return true;
            }
            if is_loading_assistant_shell(m)
                && m.text.trim().is_empty()
                && m.reasoning_text.trim().is_empty()
            {
                return true;
            }
            false
        }
    }
}

/// 返回 `messages` 中应对用户展示的原始下标（顺序不变；仅 scope 过滤，无 fuzzy dedupe）。
#[must_use]
pub fn visible_message_indices(
    messages: &[StoredMessage],
    scope: VisibleMessageScope,
) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter_map(|(idx, m)| {
            if is_message_hidden_from_view(m, messages, idx, scope) {
                None
            } else {
                Some(idx)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;
    use crate::timeline_scan::timeline_state_final_response_snapshot;

    fn msg(id: &str, role: &str, text: &str, is_tool: bool) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: role.into(),
            text: text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn chat_and_export_show_all_assistant_rows_without_fuzzy_dedupe() {
        let listing = "当前目录下有三个压缩包：\n\n1. **A** — x";
        let compact = "当前目录下有三个压缩包：\n1. **A** — x";
        let messages = vec![
            msg("u", "user", "分析", false),
            msg("a1", "assistant", listing, false),
            msg("a2", "assistant", compact, false),
        ];
        let chat = visible_message_indices(&messages, VisibleMessageScope::ChatColumn);
        let export = visible_message_indices(&messages, VisibleMessageScope::Export);
        assert_eq!(chat, export);
        assert_eq!(chat.len(), 3);
    }

    #[test]
    fn chat_and_export_skip_empty_loading_shell() {
        let messages = vec![
            msg("u", "user", "q", false),
            StoredMessage {
                id: "load".into(),
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
        ];
        // ChatColumn 和 Export 都隐藏空的 loading 壳
        assert_eq!(
            visible_message_indices(&messages, VisibleMessageScope::ChatColumn).len(),
            1
        );
        assert_eq!(
            visible_message_indices(&messages, VisibleMessageScope::Export).len(),
            1
        );
    }

    #[test]
    fn export_hides_final_response_snapshot_chat_hides_only_duplicate() {
        let body = "当前目录下有三个压缩包。";
        let messages = vec![
            msg("u", "user", "q", false),
            msg("a1", "assistant", body, false),
            StoredMessage {
                id: "snap".into(),
                role: "assistant".into(),
                text: body.to_string(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(timeline_state_final_response_snapshot()),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        let chat = visible_message_indices(&messages, VisibleMessageScope::ChatColumn);
        assert_eq!(chat.len(), 2);
        let export = visible_message_indices(&messages, VisibleMessageScope::Export);
        assert_eq!(export.len(), 2);
    }

    #[test]
    fn e2e_put_json_deserializes_snapshot_and_hides_duplicate() {
        let snap: StoredMessage = serde_json::from_str(
            r#"{"id":"snap","role":"assistant","text":"当前目录下有三个压缩包。","reasoning_text":"","state":"{\"k\":\"cm_tl\",\"t\":\"final_response_snapshot\"}"}"#,
        )
        .expect("snap json");
        assert!(
            snap.state
                .as_ref()
                .and_then(|s| s.as_timeline_parse_candidate())
                .is_some(),
            "snap state must parse as timeline JSON"
        );
        let messages = vec![
            msg("u1", "user", "分析当前目录", false),
            msg("a1", "assistant", "当前目录下有三个压缩包。", false),
            snap,
        ];
        let chat = visible_message_indices(&messages, VisibleMessageScope::ChatColumn);
        assert_eq!(chat.len(), 2, "snap duplicate must hide from chat");
        let export = visible_message_indices(&messages, VisibleMessageScope::Export);
        assert_eq!(export.len(), 2, "snap must hide from export");
    }

    #[test]
    fn chat_hides_duplicate_final_response_snapshot() {
        let body = "终答正文。";
        let messages = vec![
            msg("u", "user", "q", false),
            msg("a1", "assistant", body, false),
            StoredMessage {
                id: "snap".into(),
                role: "assistant".into(),
                text: body.to_string(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(timeline_state_final_response_snapshot()),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        let chat = visible_message_indices(&messages, VisibleMessageScope::ChatColumn);
        assert_eq!(chat.len(), 2);
        assert_eq!(messages[chat[1]].id, "a1");
    }
}
