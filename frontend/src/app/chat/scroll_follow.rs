//! 聊天跟底：**一条规则** + **两个入口**。
//!
//! - **规则**：`auto_scroll_chat` 为 true 且距底在 live edge 内时，流式内容增高用 **ΔscrollHeight** 追底（~100ms 节流）。
//! - **入口 A**（用户滚动意图）：[`super::scroll_shell`] 的 `on:wheel` / `on:scroll`。
//! - **入口 B**（主动跟底）：发送 / End 键 → [`engage_follow_and_scroll_bottom`]（全量 `scrollHeight` + 重置 anchor）。
//!
//! **注意**：`scroll_to_bottom`（发送/End 键）使用 `rAF` 确保布局完成后再读 `scrollHeight`，
//! 并以 `setTimeout(100)` 作为 Tauri 失焦时的兜底。
//! `wire_content_follow_scroll`（流式消息变化）仍使用纯 `setTimeout` 链，
//! 因为 rAF 在 Tauri/WebKitGTK 失焦/合成器暂停时可能不被调度。

use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::callback::Timeout;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use web_sys::HtmlElement;

use crate::app::chat::scroll_shell::ChatScrollShellSignals;
use crate::chat_session_state::ChatSessionSignals;
use crate::scroll_anchor::{ScrollAnchorState, follow_content_growth_by_delta};
use crate::session_ops::messages_scroller_has_non_collapsed_selection;
use crate::storage::ChatSession;

fn snap_scroll_element_to_bottom(el: &HtmlElement, baseline: RwSignal<i32>) {
    el.set_scroll_top(el.scroll_height());
    baseline.set(el.scroll_height());
}

fn scroll_element_to_bottom_if_allowed(shell: ChatScrollShellSignals) -> bool {
    if !shell.auto_scroll_chat.get_untracked() {
        return false;
    }
    let Some(el) = shell.messages_scroller.get_untracked() else {
        return false;
    };
    if messages_scroller_has_non_collapsed_selection(&el) {
        return false;
    }
    snap_scroll_element_to_bottom(&el, shell.stream_scroll_height_baseline);
    true
}

fn follow_stream_scroll_if_allowed(shell: ChatScrollShellSignals) -> bool {
    if !shell.auto_scroll_chat.get_untracked() {
        return false;
    }
    let Some(el) = shell.messages_scroller.get_untracked() else {
        return false;
    };
    if messages_scroller_has_non_collapsed_selection(&el) {
        return false;
    }
    let mut state = ScrollAnchorState {
        last_scroll_height: shell.stream_scroll_height_baseline.get_untracked(),
    };
    let followed = follow_content_growth_by_delta(&el, &mut state);
    shell
        .stream_scroll_height_baseline
        .set(state.last_scroll_height);
    followed
}

fn scroll_element_to_top(shell: ChatScrollShellSignals) {
    if let Some(el) = shell.messages_scroller.get() {
        el.set_scroll_top(0);
    }
}

/// 滚底：`setTimeout(0)` 等 Leptos DOM 批处理完成 → `rAF` 等布局完成再读 `scrollHeight`。
/// 发送/End 键时窗口聚焦，rAF 能可靠调度；`setTimeout(100)` 作为 Tauri 失焦时的兜底。
fn scroll_to_bottom(shell: ChatScrollShellSignals) {
    Timeout::new(0, move || {
        request_animation_frame(move || {
            shell.messages_scroll_from_effect.set(true);
            scroll_element_to_bottom_if_allowed(shell);
            shell.messages_scroll_from_effect.set(false);
            // 兜底：rAF 在 Tauri 失焦时可能不触发，setTimeout 保证最终滚底
            Timeout::new(100, move || {
                shell.messages_scroll_from_effect.set(true);
                scroll_element_to_bottom_if_allowed(shell);
                shell.messages_scroll_from_effect.set(false);
            })
            .forget();
        });
    })
    .forget();
}

fn scroll_to_top(shell: ChatScrollShellSignals) {
    Timeout::new(0, move || {
        shell.messages_scroll_from_effect.set(true);
        scroll_element_to_top(shell);
        shell.messages_scroll_from_effect.set(false);
        Timeout::new(50, move || {
            shell.messages_scroll_from_effect.set(true);
            scroll_element_to_top(shell);
            shell.messages_scroll_from_effect.set(false);
        })
        .forget();
    })
    .forget();
}

/// **入口 B**：开启跟底并滚到底。
pub(crate) fn engage_follow_and_scroll_bottom(shell: ChatScrollShellSignals) {
    shell.auto_scroll_chat.set(true);
    scroll_to_bottom(shell);
}

/// Home 键：关闭跟底并滚到顶。
pub(crate) fn disengage_follow_and_scroll_top(shell: ChatScrollShellSignals) {
    shell.auto_scroll_chat.set(false);
    scroll_to_top(shell);
}

fn active_session_tail_scroll_fingerprint(list: &[ChatSession], aid: &str) -> u64 {
    let Some(session) = list.iter().find(|s| s.id == aid) else {
        return 0;
    };
    let mut fp = session.messages.len() as u64;
    const TAIL: usize = 6;
    for msg in session.messages.iter().rev().take(TAIL) {
        fp = fp.wrapping_mul(41);
        fp = fp.wrapping_add(msg.id.len() as u64);
        fp = fp.wrapping_add(msg.text.len() as u64);
        fp = fp.wrapping_add(msg.reasoning_text.len() as u64);
        if let Some(st) = &msg.state {
            fp = fp.wrapping_add(st.to_wire().len() as u64);
        }
        fp = fp.wrapping_add(u64::from(msg.is_tool));
    }
    fp
}

/// **规则**接线：消息变化且跟底开启时在 live edge 用 delta 追底（~100ms 节流）。
pub(crate) fn wire_content_follow_scroll(chat: ChatSessionSignals, shell: ChatScrollShellSignals) {
    let version = Memo::new(move |_| {
        let aid = chat.active_id.get();
        let fp = chat
            .sessions
            .with(|list| active_session_tail_scroll_fingerprint(list, &aid));
        let rev = chat.stream_overlay_revision.get();
        (fp, rev)
    });
    // 节流句柄：前一次滚底未完成时不重复触发
    let pending = Rc::new(RefCell::new(false));
    Effect::new(move |_| {
        let _ = version.get();
        if !shell.auto_scroll_chat.get() {
            return;
        }
        if *pending.borrow() {
            return;
        }
        *pending.borrow_mut() = true;
        // setTimeout 等 DOM 批处理完成；+50ms 二次确认
        Timeout::new(0, {
            let pending = Rc::clone(&pending);
            move || {
                if !shell.auto_scroll_chat.get_untracked() {
                    *pending.borrow_mut() = false;
                    return;
                }
                shell.messages_scroll_from_effect.set(true);
                follow_stream_scroll_if_allowed(shell);
                shell.messages_scroll_from_effect.set(false);
                Timeout::new(50, move || {
                    shell.messages_scroll_from_effect.set(true);
                    follow_stream_scroll_if_allowed(shell);
                    shell.messages_scroll_from_effect.set(false);
                    *pending.borrow_mut() = false;
                })
                .forget();
            }
        })
        .forget();
    });
}

#[cfg(test)]
mod tests {
    use super::active_session_tail_scroll_fingerprint;
    use crate::storage::{ChatSession, StoredMessage, StoredMessageState};

    fn make_msg(id: &str, text: &str) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: "user".to_string(),
            text: text.to_string(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    fn make_session(id: &str, messages: Vec<StoredMessage>) -> ChatSession {
        ChatSession {
            id: id.to_string(),
            title: String::new(),
            draft: String::new(),
            messages,
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        }
    }

    #[test]
    fn empty_session_list_returns_zero() {
        assert_eq!(active_session_tail_scroll_fingerprint(&[], "s1"), 0);
    }

    #[test]
    fn session_not_found_returns_zero() {
        let sessions = vec![make_session("s1", vec![])];
        assert_eq!(active_session_tail_scroll_fingerprint(&sessions, "s2"), 0);
    }

    #[test]
    fn empty_messages_returns_zero() {
        let sessions = vec![make_session("s1", vec![])];
        assert_eq!(active_session_tail_scroll_fingerprint(&sessions, "s1"), 0);
    }

    #[test]
    fn single_message_returns_non_zero() {
        let sessions = vec![make_session("s1", vec![make_msg("m1", "hello")])];
        let fp = active_session_tail_scroll_fingerprint(&sessions, "s1");
        assert!(fp > 0);
    }

    #[test]
    fn same_content_produces_same_fingerprint() {
        let sessions = vec![make_session(
            "s1",
            vec![make_msg("m1", "hello"), make_msg("m2", "world")],
        )];
        let fp1 = active_session_tail_scroll_fingerprint(&sessions, "s1");
        let fp2 = active_session_tail_scroll_fingerprint(&sessions, "s1");
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn different_text_length_produces_different_fingerprint() {
        let s1 = vec![make_session("s1", vec![make_msg("m1", "hi")])];
        let s2 = vec![make_session("s1", vec![make_msg("m1", "hello")])];
        let fp1 = active_session_tail_scroll_fingerprint(&s1, "s1");
        let fp2 = active_session_tail_scroll_fingerprint(&s2, "s1");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn adding_message_changes_fingerprint() {
        let mut sessions = vec![make_session("s1", vec![make_msg("m1", "hello")])];
        let fp1 = active_session_tail_scroll_fingerprint(&sessions, "s1");
        sessions[0].messages.push(make_msg("m2", "world"));
        let fp2 = active_session_tail_scroll_fingerprint(&sessions, "s1");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn only_last_six_messages_affect_fingerprint() {
        let mut msgs: Vec<StoredMessage> = (0..10)
            .map(|i| make_msg(&format!("m{i}"), &format!("text{i}")))
            .collect();
        let sessions_tail = vec![make_session("s1", msgs.clone())];
        let fp_tail = active_session_tail_scroll_fingerprint(&sessions_tail, "s1");

        // Change the first message (outside last 6) — fingerprint should stay the same
        msgs[0] = make_msg("m0", "changed");
        let sessions_changed = vec![make_session("s1", msgs)];
        let fp_changed = active_session_tail_scroll_fingerprint(&sessions_changed, "s1");
        assert_eq!(fp_tail, fp_changed);
    }

    #[test]
    fn changing_last_message_changes_fingerprint() {
        let msgs: Vec<StoredMessage> = (0..10)
            .map(|i| make_msg(&format!("m{i}"), &format!("text{i}")))
            .collect();
        let mut sessions = vec![make_session("s1", msgs)];
        let fp1 = active_session_tail_scroll_fingerprint(&sessions, "s1");
        sessions[0].messages[9].text = "changed".to_string();
        let fp2 = active_session_tail_scroll_fingerprint(&sessions, "s1");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn different_state_produces_different_fingerprint() {
        let mut msg1 = make_msg("m1", "hello");
        msg1.state = Some(StoredMessageState::Loading);
        let mut msg2 = make_msg("m1", "hello");
        msg2.state = Some(StoredMessageState::Error);
        let fp1 = active_session_tail_scroll_fingerprint(&[make_session("s1", vec![msg1])], "s1");
        let fp2 = active_session_tail_scroll_fingerprint(&[make_session("s1", vec![msg2])], "s1");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn state_to_none_changes_fingerprint() {
        let mut msg = make_msg("m1", "hello");
        msg.state = Some(StoredMessageState::Loading);
        let fp1 =
            active_session_tail_scroll_fingerprint(&[make_session("s1", vec![msg.clone()])], "s1");
        msg.state = None;
        let fp2 = active_session_tail_scroll_fingerprint(&[make_session("s1", vec![msg])], "s1");
        assert_ne!(fp1, fp2);
    }
}
