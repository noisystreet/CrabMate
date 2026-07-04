//! 单一读路径：聊天列与 Markdown/JSON 导出共用的可见消息筛选与 fuzzy 去重。
//!
//! **写**仍经 `TurnReducer` + `sync_turn_projection`（见 `docs/Turn布局设计.md` §12.7）；
//! **读**统一经本模块，避免 UI / 导出各维护一套 skip + dedupe 规则。

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

/// 该条是否应从用户可见列表中隐藏（尚未做 assistant fuzzy dedupe）。
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

fn dedupe_assistant_indices_since_last_user(messages: &[StoredMessage], indices: &mut Vec<usize>) {
    let Some(last_user_pos) = indices.iter().rposition(|&i| messages[i].role == "user") else {
        return;
    };
    let mut kept_bodies: Vec<String> = Vec::new();
    let prefix = indices[..=last_user_pos].to_vec();
    let mut tail: Vec<usize> = Vec::new();
    for &idx in indices.iter().skip(last_user_pos + 1) {
        let m = &messages[idx];
        if m.role != "assistant" || m.is_tool {
            tail.push(idx);
            continue;
        }
        if m.state.as_ref().is_some_and(|st| st.is_loading()) {
            tail.push(idx);
            continue;
        }
        let body = m.text.clone();
        if kept_bodies
            .iter()
            .any(|prior| assistant_texts_fuzzy_duplicate(prior, body.as_str()))
        {
            continue;
        }
        kept_bodies.push(body);
        tail.push(idx);
    }
    indices.clear();
    indices.extend(prefix);
    indices.extend(tail);
}

/// 返回 `messages` 中应对用户展示的原始下标（顺序不变；自最后 user 起 fuzzy dedupe assistant）。
#[must_use]
pub fn visible_message_indices(
    messages: &[StoredMessage],
    scope: VisibleMessageScope,
) -> Vec<usize> {
    let mut indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(idx, m)| {
            if is_message_hidden_from_view(m, messages, idx, scope) {
                None
            } else {
                Some(idx)
            }
        })
        .collect();
    dedupe_assistant_indices_since_last_user(messages, &mut indices);
    indices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

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
    fn chat_and_export_share_assistant_fuzzy_dedupe() {
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
        assert_eq!(chat.len(), 2);
        assert_eq!(messages[chat[1]].id, "a1");
    }

    #[test]
    fn chat_keeps_loading_export_skips_empty_shell() {
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
        assert_eq!(
            visible_message_indices(&messages, VisibleMessageScope::ChatColumn).len(),
            2
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
                state: Some(StoredMessageState::TimelineUiJson(
                    r#"{"k":"cm_tl","t":"final_response_snapshot"}"#.into(),
                )),
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
                state: Some(StoredMessageState::TimelineUiJson(
                    r#"{"k":"cm_tl","t":"final_response_snapshot"}"#.into(),
                )),
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
