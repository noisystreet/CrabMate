//! 聊天跟底：**一条规则** + **两个入口**。
//!
//! - **规则**：`auto_scroll_chat` 为 true 时，消息内容变化则程序化滚底。
//! - **入口 A**（用户滚动意图）：[`super::scroll_shell`] 的 `on:wheel` / `on:scroll`。
//! - **入口 B**（主动跟底）：发送 / End 键 → [`engage_follow_and_scroll_bottom`]。
//!
//! **注意**：使用 `setTimeout` 链代替 `requestAnimationFrame`，
//! 因为 rAF 在 Tauri/WebKitGTK 中可能不被调度（窗口非聚焦、合成器暂停）。

use gloo_timers::callback::Timeout;
use leptos::prelude::*;

use crate::app::chat::scroll_shell::ChatScrollShellSignals;
use crate::chat_session_state::ChatSessionSignals;
use crate::session_ops::messages_scroller_has_non_collapsed_selection;
use crate::storage::ChatSession;

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
    el.set_scroll_top(el.scroll_height());
    true
}

fn scroll_element_to_top(shell: ChatScrollShellSignals) {
    if let Some(el) = shell.messages_scroller.get() {
        el.set_scroll_top(0);
    }
}

/// 滚底：`setTimeout(0)` 等 Leptos DOM 批处理完成 → `setTimeout(50)` 二次确认布局。
/// 用 `setTimeout` 链代替 rAF，因为 rAF 在 Tauri/WebKitGTK 中可能不被调度。
fn scroll_to_bottom(shell: ChatScrollShellSignals) {
    Timeout::new(0, move || {
        shell.messages_scroll_from_effect.set(true);
        scroll_element_to_bottom_if_allowed(shell);
        shell.messages_scroll_from_effect.set(false);
        // 二次确认：等浏览器完成布局（rAF 在 WebKitGTK 可能不触发）
        Timeout::new(50, move || {
            shell.messages_scroll_from_effect.set(true);
            scroll_element_to_bottom_if_allowed(shell);
            shell.messages_scroll_from_effect.set(false);
        })
        .forget();
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

/// **规则**接线：消息变化且跟底开启时滚底。
pub(crate) fn wire_content_follow_scroll(chat: ChatSessionSignals, shell: ChatScrollShellSignals) {
    let version = Memo::new(move |_| {
        let aid = chat.active_id.get();
        let fp = chat
            .sessions
            .with(|list| active_session_tail_scroll_fingerprint(list, &aid));
        let rev = chat.stream_overlay_revision.get();
        (fp, rev)
    });
    Effect::new(move |_| {
        let _ = version.get();
        if shell.auto_scroll_chat.get() {
            scroll_to_bottom(shell);
        }
    });
}
