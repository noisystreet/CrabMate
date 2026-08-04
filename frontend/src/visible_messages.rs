//! 单一读路径：导出与 TUI 共用的可见消息筛选。
//!
//! **写**经 `TurnReducer` + `sync_turn_projection`（见 `docs/Turn布局设计.md` §12.7）；
//! **读**：导出经 [`visible_message_indices_for_export`]；TUI 主列经 [`tui_should_render_message`]。
//!
//! - 主列 / 导出均跳过 [`is_ephemeral_timeline_assistant_for_chat_ui`] 与空助手壳；
//! - 导出另跳过 [`is_ephemeral_timeline_assistant_for_export`] 的规划轮 JSON / 参数残留；
//! - TUI 对空壳另允许 stream overlay 有正文时挂载。

use crate::storage::StoredMessage;
use crate::timeline_scan::{
    is_ephemeral_timeline_assistant_for_chat_ui, is_ephemeral_timeline_assistant_for_export,
};

/// 导出路径是否应隐藏该条。
fn is_message_hidden_from_export(m: &StoredMessage, messages: &[StoredMessage]) -> bool {
    if is_ephemeral_timeline_assistant_for_export(m, messages) {
        return true;
    }
    is_empty_assistant_body(m)
}

/// 空助手正文（无工具）：含空 Loading 壳；导出与 TUI 均应隐藏（TUI 在有 overlay 时可显示）。
#[must_use]
pub(crate) fn is_empty_assistant_body(m: &StoredMessage) -> bool {
    !m.is_tool
        && m.role == "assistant"
        && m.text.trim().is_empty()
        && m.reasoning_text.trim().is_empty()
}

/// TUI 主列是否应挂载该消息。
///
/// - 工具 / 非助手行：始终挂载；
/// - 主列 ephemeral 助手旁注（与导出共享子集）：隐藏；
/// - 空助手壳：仅在有 stream overlay 正文时显示。
#[must_use]
pub(crate) fn tui_should_render_message(
    m: &StoredMessage,
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&crate::stream_text_overlay::StreamTextOverlay>,
) -> bool {
    if m.is_tool || m.role != "assistant" {
        return true;
    }
    if is_ephemeral_timeline_assistant_for_chat_ui(m, messages) {
        return false;
    }
    if !is_empty_assistant_body(m) {
        return true;
    }
    let show = overlay.is_some_and(|o| {
        o.session_id == session_id
            && o.message_id == m.id
            && (!o.answer.trim().is_empty() || !o.reasoning.trim().is_empty())
    });
    if !show {
        crate::layout_debug_counters::note_empty_shell_skip();
    }
    show
}

/// 返回 `messages` 中应对**导出**展示的原始下标（顺序不变）。
#[must_use]
pub fn visible_message_indices_for_export(messages: &[StoredMessage]) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter_map(|(idx, m)| {
            if is_message_hidden_from_export(m, messages) {
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
    use crate::stream_text_overlay::StreamTextOverlay;
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

    fn tui_visible_ids(messages: &[StoredMessage]) -> Vec<&str> {
        messages
            .iter()
            .filter(|m| tui_should_render_message(m, messages, "s", None))
            .map(|m| m.id.as_str())
            .collect()
    }

    #[test]
    fn export_and_tui_keep_fuzzy_near_duplicate_assistants() {
        let listing = "当前目录下有三个压缩包：\n\n1. **A** — x";
        let compact = "当前目录下有三个压缩包：\n1. **A** — x";
        let messages = vec![
            msg("u", "user", "分析", false),
            msg("a1", "assistant", listing, false),
            msg("a2", "assistant", compact, false),
        ];
        let export = visible_message_indices_for_export(&messages);
        assert_eq!(export.len(), 3);
        assert_eq!(tui_visible_ids(&messages), ["u", "a1", "a2"]);
    }

    #[test]
    fn export_and_tui_skip_empty_loading_shell() {
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
        assert_eq!(visible_message_indices_for_export(&messages).len(), 1);
        assert_eq!(tui_visible_ids(&messages), ["u"]);
    }

    #[test]
    fn tui_renders_empty_loading_when_overlay_has_answer() {
        let load = StoredMessage {
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
        };
        let messages = [load.clone()];
        let overlay = StreamTextOverlay {
            session_id: "s1".into(),
            message_id: "load".into(),
            answer: "流式中".into(),
            reasoning: String::new(),
        };
        assert!(tui_should_render_message(
            &load,
            &messages,
            "s1",
            Some(&overlay)
        ));
    }

    #[test]
    fn export_and_tui_hide_final_response_snapshot() {
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
        assert_eq!(visible_message_indices_for_export(&messages).len(), 2);
        assert_eq!(tui_visible_ids(&messages), ["u", "a1"]);
    }

    #[test]
    fn e2e_put_json_deserializes_snapshot_and_hides_from_tui_and_export() {
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
        assert_eq!(
            visible_message_indices_for_export(&messages).len(),
            2,
            "snap must hide from export"
        );
        assert_eq!(
            tui_visible_ids(&messages),
            ["u1", "a1"],
            "snap must hide from TUI"
        );
    }

    #[test]
    fn export_and_tui_hide_commentary_before_tools() {
        let messages = vec![
            msg("u", "user", "q", false),
            StoredMessage {
                id: "c1".into(),
                role: "assistant".into(),
                text: "先说明一下".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::CommentaryBeforeTools),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            msg("a1", "assistant", "终答", false),
        ];
        assert_eq!(visible_message_indices_for_export(&messages).len(), 2);
        assert_eq!(tui_visible_ids(&messages), ["u", "a1"]);
    }

    #[test]
    fn export_and_tui_hide_orchestration_route() {
        let messages = vec![
            msg("u", "user", "q", false),
            msg("r1", "assistant", "编排路由：direct", false),
            msg("a1", "assistant", "终答", false),
        ];
        assert_eq!(visible_message_indices_for_export(&messages).len(), 2);
        assert_eq!(tui_visible_ids(&messages), ["u", "a1"]);
    }

    #[test]
    fn tui_keeps_staged_planner_that_export_drops() {
        let plan = r#"{"type":"agent_reply_plan","version":1,"steps":[]}"#;
        let messages = vec![
            msg("u", "user", "q", false),
            msg("p1", "assistant", plan, false),
            msg("a1", "assistant", "终答", false),
        ];
        assert_eq!(
            visible_message_indices_for_export(&messages).len(),
            2,
            "export drops staged planner JSON"
        );
        assert_eq!(
            tui_visible_ids(&messages),
            ["u", "p1", "a1"],
            "TUI still shows planner round for in-chat formatting"
        );
    }
}
