//! 用户主动中止进行中的 **`/chat/stream`**：将 **`AbortController`**、壳层 **`status_busy` / `tool_busy`**
//! 与会话内 **assistant / 工具** 的 **`Loading`** 占位收口到 **[`apply_user_abort_of_inflight_stream`]**，
//! 避免接线层散落「只清信号、不改消息」的隐式分裂。
//!
//! **与其它收尾路径的关系**（与 [`crate::chat_session_state::make_chat_stream_busy_memos`] 同源）：
//! - **正常结束**：`tool_busy` 与工具时间线占位通常已由 SSE（如 `tool_result`）消化；`on_done` 再回落壳层 busy。
//! - **用户中止**：本模块 [`apply_user_abort_of_inflight_stream`] 同时收口助手/工具 **`Loading`** 行并回落壳层双忙（`StreamShellBusyOp::ReleaseTurnShellBusy`）。
//! - **SSE/HTTP 错误**：`on_error` 对会话 `messages` 的写回应经 `callbacks::error_session::apply_stream_error_on_messages`（助手尾泡 + **`Loading`** 工具行），不能只清 `tool_busy`，否则时间线卡与谓词长期不一致。
//!
//! 会话目标与 SSE 写入一致：使用 [`crate::chat_session_state::ChatSessionSignals::effective_stream_message_session_id`]。

use leptos::prelude::{GetUntracked, Set};

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n;
use crate::i18n::Locale;
use crate::storage::StoredMessage;
use crate::stream_text_overlay::stream_overlay_take_into_stored_message;

use super::composer_stream::{clear_abort_slot, user_cancel_in_flight_stream};
use super::handles::ComposerStreamShell;

/// 新一轮 `/chat/stream` 已排队且已 `push` 新尾条 `loading` 助手时，将**同会话内**其它仍处 `loading` 的助手占位收口为「已中断」，
/// 避免上一轮被 `abort` 后迟到回调与 [`crate::chat_session_state::ChatSessionSignals::stream_attach_generation_untracked`] 门闩叠加留下僵尸尾泡。
pub(crate) fn finalize_superseded_assistant_loading_rows_except(
    chat: ChatSessionSignals,
    session_id: &str,
    keep_asst_id: &str,
    loc: Locale,
) {
    chat.update_sessions_composer(|list| {
        let Some(session) = list.iter_mut().find(|s| s.id == session_id) else {
            return;
        };
        for m in session.messages.iter_mut() {
            if m.role != "assistant" || m.is_tool {
                continue;
            }
            if !m.state.as_ref().is_some_and(|st| st.is_loading()) {
                continue;
            }
            if m.id == keep_asst_id {
                continue;
            }
            let mid = m.id.clone();
            stream_overlay_take_into_stored_message(
                chat.stream_text_overlay,
                session_id,
                mid.as_str(),
                m,
            );
            m.state = None;
            if m.text.trim().is_empty() && m.reasoning_text.trim().is_empty() {
                m.text = i18n::stream_stopped_inline(loc).to_string();
            } else {
                m.text.push_str(i18n::stream_stopped_suffix(loc));
            }
        }
    });
}

/// 单轮流式 UI 是否仍视为「在途」：`status_busy` / `tool_busy`、`AbortController` 槽位已占、或有效会话内仍有 **Loading** 助手/工具占位。
/// 与 [`crate::chat_session_state::make_chat_stream_busy_memos`] 的 **`stream_turn_busy_ui`** 同源 OR（此处用 `get_untracked` 供非响应式路径）。
#[must_use]
pub(crate) fn stream_ui_inflight_untracked(
    chat: ChatSessionSignals,
    shell: &ComposerStreamShell,
) -> bool {
    if shell.stream.status_busy.get_untracked() || shell.stream.tool_busy.get_untracked() {
        return true;
    }
    if shell.stream.abort_cell.lock().unwrap().is_some() {
        return true;
    }
    crate::chat_session_state::session_has_stream_loading_placeholders_untracked(chat)
}

/// 用户从 Web 主列点击「停止」时的**唯一**收口（`cancel_stream` 闭包仅调用此处）。
///
/// 1. 若 [`stream_ui_inflight_untracked`] 为假：无操作，返回 `false`。
/// 2. 否则：尽力 `abort` 在途 HTTP（见 [`user_cancel_in_flight_stream`]），并在 [`ChatSessionSignals::effective_stream_message_session_id`] 上收口 `Loading` 占位，回落 **`status_busy` / `tool_busy`**，最后 [`clear_abort_slot`]。
///
/// 「整轮在途」谓词与 **`stream_turn_busy_ui`** 一致。
#[must_use]
pub(crate) fn apply_user_abort_of_inflight_stream(
    chat: ChatSessionSignals,
    shell: &ComposerStreamShell,
    loc: Locale,
) -> bool {
    if !stream_ui_inflight_untracked(chat, shell) {
        return false;
    }
    let _ = user_cancel_in_flight_stream(shell);
    let sid = chat.effective_stream_message_session_id();
    finalize_loading_placeholders_after_user_abort_on_session(chat, &sid, loc);
    let attach_gen = chat.stream_attach_generation_untracked();
    shell.stream.apply_release_turn_and_stream_run(attach_gen);
    clear_abort_slot(shell);
    true
}

fn finalize_loading_placeholders_after_user_abort_on_session(
    chat: ChatSessionSignals,
    session_id: &str,
    loc: Locale,
) {
    chat.update_sessions_composer(|list| {
        let Some(s) = list.iter_mut().find(|s| s.id == session_id) else {
            return;
        };
        if let Some(m) = s.messages.iter_mut().rev().find(|m| {
            m.role == "assistant"
                && !m.is_tool
                && m.state.as_ref().is_some_and(|st| st.is_loading())
        }) {
            let mid_flush = m.id.clone();
            stream_overlay_take_into_stored_message(
                chat.stream_text_overlay,
                session_id,
                mid_flush.as_str(),
                m,
            );
        }
        apply_abort_finalization_to_messages(&mut s.messages, loc);
    });
    chat.stream_text_overlay.set(None);
}

fn apply_abort_finalization_to_messages(messages: &mut [StoredMessage], loc: Locale) {
    if let Some(m) = messages.iter_mut().rev().find(|m| {
        m.role == "assistant" && !m.is_tool && m.state.as_ref().is_some_and(|st| st.is_loading())
    }) {
        m.state = None;
        if m.text.trim().is_empty() {
            m.text = i18n::stream_stopped_inline(loc).to_string();
        } else {
            m.text.push_str(i18n::stream_stopped_suffix(loc));
        }
    }
    finalize_loading_tool_placeholders_to_stopped(messages, loc);
}

/// 将仍处 `Loading` 的工具时间线占位收口为「已中断」展示（与用户点击停止的文案/语义对齐）。
///
/// **调用方**：用户中止经 [`apply_abort_finalization_to_messages`] 间接调用；流式错误路径由
/// `callbacks::error_session::apply_stream_error_on_messages` 在写回尾助手错误时**一并**调用本函数。
/// 若只把 [`ComposerStreamShell`](super::handles::ComposerStreamShell) 上的 `tool_busy` 置假而不改消息，
/// `Loading` 工具泡仍会使 [`crate::chat_session_state::session_has_loading_tool_message`] 长期为真，状态栏/停止按钮语义卡住。
pub(crate) fn finalize_loading_tool_placeholders_to_stopped(
    messages: &mut [StoredMessage],
    loc: Locale,
) {
    let running_label = i18n::status_tool_running(loc);
    let stopped_tool = i18n::status_tool_stopped_user(loc);
    for m in messages.iter_mut() {
        if !m.is_tool || !m.state.as_ref().is_some_and(|st| st.is_loading()) {
            continue;
        }
        m.state = None;
        if m.reasoning_text.contains("status: running") {
            m.reasoning_text = m
                .reasoning_text
                .replace("status: running", "status: stopped (user)");
        }
        if m.text.contains(running_label) {
            m.text = m.text.replacen(running_label, stopped_tool, 1);
        } else if m.text.trim().is_empty() {
            m.text = i18n::stream_stopped_inline(loc).to_string();
        } else {
            m.text.push_str(i18n::stream_stopped_suffix(loc));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_abort_finalization_to_messages;
    use crate::i18n::Locale;
    use crate::storage::{StoredMessage, StoredMessageState};

    fn loading_tool(text: &str) -> StoredMessage {
        StoredMessage {
            id: "t1".to_string(),
            role: "system".to_string(),
            text: text.to_string(),
            reasoning_text: "tool: x\nstatus: running".to_string(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: true,
            tool_call_id: None,
            tool_name: Some("git".to_string()),
            created_at: 0,
        }
    }

    #[test]
    fn abort_clears_tool_loading_and_replaces_running_detail() {
        let mut msgs = vec![loading_tool("摘要 · 工具执行中…")];
        apply_abort_finalization_to_messages(&mut msgs, Locale::ZhHans);
        let m = &msgs[0];
        assert!(!m.state.as_ref().is_some_and(|s| s.is_loading()));
        assert!(m.reasoning_text.contains("stopped"));
        assert!(m.text.contains("已终止") || m.text.contains("Stopped"));
    }
}
