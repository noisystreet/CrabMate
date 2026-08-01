//! 用户主动中止进行中的 **`/chat/stream`**：将 **`AbortController`**、
//! [`TurnLifecycle`](crate::app::turn_lifecycle) 与会话内 **assistant / 工具** 的 **`Loading`** 占位收口到 **[`apply_user_abort_of_inflight_stream`]**，
//! 避免接线层散落「只清信号、不改消息」的隐式分裂。
//!
//! **与其它收尾路径的关系**（与 [`crate::chat_session_state::make_chat_stream_busy_memos`] 同源）：
//! - **正常结束**：工具时间线占位通常已由 SSE（如 `tool_result`）消化；`on_done` 再 [`ShellReleased`](crate::app::turn_lifecycle::TurnLifecycleEvent::ShellReleased) 并清 abort 槽。
//! - **用户中止**：本模块 [`apply_user_abort_of_inflight_stream`] 同时收口助手/工具 **`Loading`** 行并 dispatch lifecycle 收尾。
//! - **SSE/HTTP 错误**：`on_error` 对会话 `messages` 的写回应经 `callbacks::error_session::apply_stream_error_on_messages`（助手尾泡 + **`Loading`** 工具行），不能只清 lifecycle，否则时间线卡与谓词长期不一致。
//!
//! 会话目标与 SSE 写入一致：使用 [`crate::chat_session_state::ChatSessionSignals::effective_stream_message_session_id`]。

use crate::app::turn_lifecycle::turn_lifecycle_stream_turn_busy;
use crate::chat_session_state::{ChatSessionSignals, session_has_loading_tool_message};
use crate::i18n;
use crate::i18n::Locale;
use crate::message_loading::{
    ToolLoadingFinalizeKind, is_loading_plain_assistant, messages_have_loading_tool,
};
use crate::storage::StoredMessage;
use crate::stream_text_overlay::stream_overlay_take_into_stored_message;
use leptos::prelude::GetUntracked;

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
            if !is_loading_plain_assistant(m) {
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

/// 单轮流式 UI 是否仍视为「在途」；与 [`crate::chat_session_state::make_chat_stream_busy_memos`] 的 **`stream_turn_busy_ui`** 同源（`get_untracked`）。
#[must_use]
pub(crate) fn stream_ui_inflight_untracked(
    chat: ChatSessionSignals,
    shell: &ComposerStreamShell,
) -> bool {
    let _ = chat;
    turn_lifecycle_stream_turn_busy(
        shell.stream.turn_lifecycle.get_untracked(),
        shell.stream.abort_cell.lock().unwrap().is_some(),
    )
}

/// 用户从 Web 主列点击「停止」时的**唯一**收口（`cancel_stream` 闭包仅调用此处）。
///
/// 1. 若 [`stream_ui_inflight_untracked`] 为真：尽力 `abort` 在途 HTTP，收口助手/工具 `Loading`，dispatch lifecycle 收尾。
/// 2. 否则若仍有僵尸工具 `Loading`（流已结束未配对 `tool_result`、或重启后残留）：仅收口工具占位，返回 `true`。
/// 3. 皆无则返回 `false`。
///
/// 「整轮在途」谓词与 **`stream_turn_busy_ui`** 一致。
#[must_use]
pub(crate) fn apply_user_abort_of_inflight_stream(
    chat: ChatSessionSignals,
    shell: &ComposerStreamShell,
    loc: Locale,
) -> bool {
    if stream_ui_inflight_untracked(chat, shell) {
        let _ = user_cancel_in_flight_stream(shell);
        let sid = chat.effective_stream_message_session_id();
        finalize_loading_placeholders_after_user_abort_on_session(chat, &sid, loc);
        let attach_gen = chat.stream_attach_generation_untracked();
        shell.stream.dispatch_turn_lifecycle(
            crate::app::chat::turn_lifecycle::TurnLifecycleEvent::UserAbortRequested {
                attach_generation: attach_gen,
            },
        );
        shell.stream.apply_release_turn_and_stream_run(attach_gen);
        clear_abort_slot(shell);
        return true;
    }
    if !session_has_loading_tool_message(chat) {
        return false;
    }
    let sid = chat.effective_stream_message_session_id();
    chat.update_sessions_composer(|list| {
        let Some(s) = list.iter_mut().find(|sess| sess.id == sid) else {
            return;
        };
        if !messages_have_loading_tool(&s.messages) {
            return;
        }
        // 用户主动点「停止」：即使用户文案「已终止」，勿标成重启语义的 OrphanStale。
        crate::message_loading::finalize_loading_tool_placeholders(
            &mut s.messages,
            loc,
            ToolLoadingFinalizeKind::UserStopped,
        );
    });
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
        if let Some(m) = s
            .messages
            .iter_mut()
            .rev()
            .find(|m| is_loading_plain_assistant(m))
        {
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
    chat.clear_stream_text_overlay();
}

fn apply_abort_finalization_to_messages(messages: &mut [StoredMessage], loc: Locale) {
    if let Some(m) = messages
        .iter_mut()
        .rev()
        .find(|m| is_loading_plain_assistant(m))
    {
        m.state = None;
        if m.text.trim().is_empty() {
            m.text = i18n::stream_stopped_inline(loc).to_string();
        } else {
            m.text.push_str(i18n::stream_stopped_suffix(loc));
        }
    }
    finalize_loading_tool_placeholders_to_stopped(messages, loc);
}

/// 将仍处 `Loading` 的工具时间线占位收口为「已终止」展示（与用户点击停止的文案/语义对齐）。
///
/// **调用方**：用户中止经 [`apply_abort_finalization_to_messages`] 间接调用；流式错误路径由
/// `callbacks::error_session::apply_stream_error_on_messages` 在写回尾助手错误时**一并**调用本函数。
/// 若只 dispatch lifecycle 收尾而不改消息，
/// `Loading` 工具泡仍会使 [`crate::chat_session_state::session_has_loading_tool_message`] 长期为真，状态栏/停止按钮语义卡住。
pub(crate) fn finalize_loading_tool_placeholders_to_stopped(
    messages: &mut [StoredMessage],
    loc: Locale,
) {
    crate::message_loading::finalize_loading_tool_placeholders(
        messages,
        loc,
        crate::message_loading::ToolLoadingFinalizeKind::UserStopped,
    );
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

    #[test]
    fn orphan_stale_finalize_marks_interrupted() {
        use crate::message_loading::{ToolLoadingFinalizeKind, finalize_loading_tool_placeholders};
        let mut msgs = vec![loading_tool("工具：http_fetch")];
        finalize_loading_tool_placeholders(
            &mut msgs,
            Locale::ZhHans,
            ToolLoadingFinalizeKind::OrphanStale,
        );
        assert!(!msgs[0].state.as_ref().is_some_and(|s| s.is_loading()));
        assert!(msgs[0].reasoning_text.contains("interrupted (stale)"));
        assert!(msgs[0].text.contains("已中断"));
    }
}
