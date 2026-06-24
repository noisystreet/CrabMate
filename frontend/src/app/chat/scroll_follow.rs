//! 聊天跟底：**一条规则** + **两个入口**。
//!
//! - **规则**：`auto_scroll_chat` 为 true 时，消息内容变化则程序化滚底；为 false 则不滚。
//! - **入口 A**（用户滚动意图）：[`super::scroll_shell`] 的 `on:wheel` / `on:scroll`。
//! - **入口 B**（主动跟底）：发送、流式再生、End 键等调用 [`engage_follow_and_scroll_bottom`]。

use std::sync::atomic::{AtomicBool, Ordering};

use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::app::chat::scroll_shell::ChatScrollShellSignals;
use crate::app::scroll_guard::MessagesScrollFromEffectGuard;
use crate::chat_session_state::ChatSessionSignals;
use crate::session_ops::messages_scroller_has_non_collapsed_selection;
use crate::storage::ChatSession;

/// 滚底脉冲合并：发送 / End / 流式 Effect 共用单条在飞任务。
static FOLLOW_PULSE_TO_BOTTOM_PENDING: AtomicBool = AtomicBool::new(false);

/// 流式增量布局对齐：与 End 键共享的三次脉冲间隔（ms）。
const PULSE_DELAYS_MS: [u32; 3] = [0, 0, 16];

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

/// 三次脉冲滚底（发送、流式、End 共用调度器）。
fn schedule_pulse_to_bottom(shell: ChatScrollShellSignals) {
    if FOLLOW_PULSE_TO_BOTTOM_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    spawn_local(async move {
        let clear_pending = || FOLLOW_PULSE_TO_BOTTOM_PENDING.store(false, Ordering::Release);
        let _guard = MessagesScrollFromEffectGuard::new(shell.messages_scroll_from_effect);
        for delay in PULSE_DELAYS_MS {
            TimeoutFuture::new(delay).await;
            if !shell.auto_scroll_chat.get_untracked() {
                clear_pending();
                return;
            }
            let _ = scroll_element_to_bottom_if_allowed(shell);
        }
        clear_pending();
    });
}

fn schedule_pulse_to_top(shell: ChatScrollShellSignals) {
    spawn_local(async move {
        let _guard = MessagesScrollFromEffectGuard::new(shell.messages_scroll_from_effect);
        for delay in PULSE_DELAYS_MS {
            TimeoutFuture::new(delay).await;
            scroll_element_to_top(shell);
        }
    });
}

/// **入口 B**：开启跟底并脉冲滚到底（发送、流式再生、End 键等）。
pub(crate) fn engage_follow_and_scroll_bottom(shell: ChatScrollShellSignals) {
    shell.auto_scroll_chat.set(true);
    schedule_pulse_to_bottom(shell);
}

/// Home 键：关闭跟底并脉冲滚到顶。
pub(crate) fn disengage_follow_and_scroll_top(shell: ChatScrollShellSignals) {
    shell.auto_scroll_chat.set(false);
    schedule_pulse_to_top(shell);
}

/// 跟底指纹：只看活跃会话尾部若干条，避免流式时对整页消息 `fold` 全文长度。
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

/// **规则**接线：消息指纹或流式 overlay 变化且 `auto_scroll_chat` 为 true 时脉冲跟底。
pub(crate) fn wire_content_follow_scroll(chat: ChatSessionSignals, shell: ChatScrollShellSignals) {
    let sessions = chat.sessions;
    let active_id = chat.active_id;
    let stream_text_overlay = chat.stream_text_overlay;
    Effect::new(move |_| {
        let aid = active_id.get();
        let _fingerprint = sessions.with(|list| active_session_tail_scroll_fingerprint(list, &aid));
        let _overlay = stream_text_overlay.get();

        if !shell.auto_scroll_chat.get() {
            return;
        }
        schedule_pulse_to_bottom(shell);
    });
}
