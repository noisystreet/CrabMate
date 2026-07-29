//! 工具前旁注 / 终答投影后的 loading 所有权移交（I14，**兼容路径**）。
//!
//! Phase B+ 主路径旁白直写 `turn-commentary-*`，不经 `loading.text`。本模块仅处理：
//! 残留非空 `loading.text` 与定稿行同文时的清空，以及定稿后清 overlay。
//! 见 [`super::text_ownership`]。

use leptos::prelude::GetUntracked;

use crate::message_loading::is_loading_plain_assistant;
use crate::storage::StoredMessage;
use crate::stream_text_overlay::{
    stream_overlay_answer_for_message, stream_overlay_clear_answer_for_message,
};

use super::super::super::context::ChatStreamCallbackCtx;

/// 除 `loading_idx` 外，是否已有定稿助手行持有与 `live_trim` 完全相同的正文。
pub(super) fn persisted_assistant_owns_live_text(
    messages: &[StoredMessage],
    loading_idx: usize,
    live_trim: &str,
) -> bool {
    if live_trim.is_empty() {
        return false;
    }
    messages.iter().enumerate().any(|(i, m)| {
        i != loading_idx
            && m.role == "assistant"
            && !m.is_tool
            && !is_loading_plain_assistant(m)
            && m.text.trim() == live_trim
    })
}

/// 只读：任意定稿助手行是否已持有 `live_text`（不排除 loading 自身 id）。
pub(super) fn persisted_assistant_owns_live_text_any(
    messages: &[StoredMessage],
    live_text: &str,
) -> bool {
    let live_trim = live_text.trim();
    if live_trim.is_empty() {
        return false;
    }
    messages.iter().any(|m| {
        m.role == "assistant"
            && !m.is_tool
            && !is_loading_plain_assistant(m)
            && m.text.trim() == live_trim
    })
}

/// 若 loading 尾泡 **stored** 正文已与某条定稿助手行同文，清空该尾泡 `text` 并返回 `true`。
///
/// - **兼容路径**：仅当 `loading.text` 非空且与定稿同文时清空，并计入 `commentary_handoff`。
/// - **主路径**（Phase B+）：旁白不写 `loading.text`；若 stored 已空但 overlay 与定稿同文，
///   仍返回 `true` 供调用方清 overlay，**不**计入 handoff（overlay ≡ active 缓存收口）。
///
/// 必须在 reconciler flush **之后**、同一 `messages` 更新内调用。
pub(super) fn clear_loading_tail_text_if_persisted_owns(
    messages: &mut [StoredMessage],
    loading_tail_id: Option<&str>,
    overlay_answer: Option<&str>,
) -> bool {
    let Some(mid) = loading_tail_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some(idx) = messages.iter().position(|m| m.id == mid) else {
        return false;
    };
    let stored = messages[idx].text.as_str();
    let stored_nonempty = !stored.trim().is_empty();
    let live = if stored_nonempty {
        stored
    } else {
        overlay_answer.unwrap_or("")
    };
    let live_trim = live.trim();
    if live_trim.is_empty() {
        return false;
    }
    if !persisted_assistant_owns_live_text(messages, idx, live_trim) {
        return false;
    }
    if stored_nonempty {
        messages[idx].text.clear();
        crate::layout_debug_counters::note_commentary_handoff();
    }
    true
}

/// 会话更新外：若定稿助手行已拥有当前 overlay/stored 正文，清空 overlay（幂等）。
pub(super) fn clear_overlay_if_commentary_owns_live(stream_ctx: &ChatStreamCallbackCtx) {
    let mid = stream_ctx.scratch.clone_assistant_id();
    let sid = stream_ctx.bound_stream_session_id.clone();
    let overlay = stream_overlay_answer_for_message(
        stream_ctx.chat.stream_text_overlay.get_untracked().as_ref(),
        sid.as_str(),
        mid.as_str(),
    );
    let stored_text = stream_ctx
        .read_bound_session(|s| {
            s.messages
                .iter()
                .find(|m| m.id == mid.as_str())
                .map(|m| m.text.clone())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let live = if !stored_text.trim().is_empty() {
        stored_text
    } else {
        overlay.unwrap_or_default()
    };
    if live.trim().is_empty() {
        // stored 已空时仍可能残留 overlay（同文已在定稿行）。
        let Some(overlay_text) = stream_overlay_answer_for_message(
            stream_ctx.chat.stream_text_overlay.get_untracked().as_ref(),
            sid.as_str(),
            mid.as_str(),
        ) else {
            return;
        };
        let owns = stream_ctx
            .read_bound_session(|s| {
                persisted_assistant_owns_live_text_any(&s.messages, overlay_text.as_str())
            })
            .unwrap_or(false);
        if owns {
            stream_overlay_clear_answer_for_message(
                stream_ctx.chat.stream_text_overlay,
                sid.as_str(),
                mid.as_str(),
                Some(stream_ctx.chat.stream_overlay_revision),
            );
        }
        return;
    }
    let owns = stream_ctx
        .read_bound_session(|s| persisted_assistant_owns_live_text_any(&s.messages, live.as_str()))
        .unwrap_or(false);
    if !owns {
        return;
    }
    stream_overlay_clear_answer_for_message(
        stream_ctx.chat.stream_text_overlay,
        sid.as_str(),
        mid.as_str(),
        Some(stream_ctx.chat.stream_overlay_revision),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

    fn msg(id: &str, text: &str, loading: bool) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "assistant".into(),
            text: text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: loading.then_some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn clears_loading_when_same_text_on_commentary() {
        let mut messages = vec![
            msg("turn-commentary-tc1", "旁白。", false),
            msg("load", "旁白。", true),
        ];
        assert!(clear_loading_tail_text_if_persisted_owns(
            &mut messages,
            Some("load"),
            None
        ));
        assert!(messages[1].text.is_empty());
    }

    #[test]
    fn clears_loading_when_same_text_on_final_answer() {
        let mut messages = vec![
            msg("turn-final-answer", "中间旁白误入终答。", false),
            msg("load", "中间旁白误入终答。", true),
        ];
        assert!(clear_loading_tail_text_if_persisted_owns(
            &mut messages,
            Some("load"),
            None
        ));
        assert!(messages[1].text.is_empty());
    }

    #[test]
    fn keeps_loading_when_only_prior_turn_commentary_differs() {
        let mut messages = vec![
            msg("turn-commentary-old", "上轮旁白。", false),
            msg("load", "本轮旁白。", true),
        ];
        assert!(!clear_loading_tail_text_if_persisted_owns(
            &mut messages,
            Some("load"),
            None
        ));
        assert_eq!(messages[1].text, "本轮旁白。");
    }

    #[test]
    fn clears_loading_using_overlay_when_stored_empty() {
        let mut messages = vec![
            msg("turn-commentary-tc1", "仅 overlay。", false),
            msg("load", "", true),
        ];
        // stored 已空：仍信号清 overlay，但不写入/计数 loading.text handoff。
        assert!(clear_loading_tail_text_if_persisted_owns(
            &mut messages,
            Some("load"),
            Some("仅 overlay。")
        ));
        assert!(messages[1].text.is_empty());
    }

    #[test]
    fn overlay_only_match_does_not_require_nonempty_loading() {
        let mut messages = vec![
            msg("turn-final-answer", "终答。", false),
            msg("load", "", true),
        ];
        assert!(clear_loading_tail_text_if_persisted_owns(
            &mut messages,
            Some("load"),
            Some("终答。")
        ));
        assert!(messages[1].text.is_empty());
    }
}
