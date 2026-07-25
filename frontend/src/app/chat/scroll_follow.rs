//! StickToBottom **内容侧**：Pinned 时内容变高则 snap 到底。
//!
//! 状态图与用户意图（wheel / pointer / 近底 re-pin）见 [`super::scroll_shell`]。
//!
//! | API | 作用 |
//! |-----|------|
//! | [`engage_follow_and_scroll_bottom`] | 发送 / End → pin + 滚底（即 engage_on_user_send） |
//! | [`on_content_resize_if_pinned`] / [`follow_after_content_paint`] | ResizeObserver / DOM paint 后若仍 pin 则贴底 |
//! | [`wire_content_follow_scroll`] | 会话尾指纹 / overlay revision 变化时若 pin 则 rAF 贴底 |
//! | [`disengage_follow_and_scroll_top`] | Home → unpin + 滚顶 |

use gloo_timers::callback::Timeout;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;

use crate::app::chat::scroll_shell::{ChatScrollShellSignals, stick_pin, stick_unpin};
use crate::chat_session_state::ChatSessionSignals;
use crate::session_ops::messages_scroller_has_non_collapsed_selection;
use crate::storage::ChatSession;

fn snap_to_bottom(shell: ChatScrollShellSignals) {
    let Some(el) = shell.messages_scroller.get_untracked() else {
        return;
    };
    if messages_scroller_has_non_collapsed_selection(&el) {
        return;
    }
    el.set_scroll_top(el.scroll_height());
}

/// 跟底仍开启时 snap 到底；延迟任务执行前必须重新检查用户意图。
fn snap_to_bottom_if_following(shell: ChatScrollShellSignals) {
    if shell.auto_scroll_chat.get_untracked() {
        snap_to_bottom(shell);
    }
}

/// 滚底：rAF 等布局完成；setTimeout 作 Tauri 失焦兜底。
fn scroll_to_bottom(shell: ChatScrollShellSignals) {
    request_animation_frame(move || {
        snap_to_bottom_if_following(shell);
        // Tauri/WebKitGTK 失焦时 rAF 可能不触发，setTimeout 兜底
        Timeout::new(100, move || {
            snap_to_bottom_if_following(shell);
        })
        .forget();
    });
}

fn scroll_to_top(shell: ChatScrollShellSignals) {
    request_animation_frame(move || {
        if let Some(el) = shell.messages_scroller.get() {
            el.set_scroll_top(0);
        }
        Timeout::new(50, move || {
            if let Some(el) = shell.messages_scroller.get() {
                el.set_scroll_top(0);
            }
        })
        .forget();
    });
}

/// 用户发送 / End：进入 Pinned 并滚到底。
///
/// setTimeout(0) + rAF + setTimeout(100) 三重确保：
/// - setTimeout(0)：等 Leptos DOM 批处理完成后再 snap，避免提前读到旧 scrollHeight
/// - rAF：布局完成后最终位置
/// - setTimeout(100)：Tauri 失焦兜底
pub(crate) fn engage_follow_and_scroll_bottom(shell: ChatScrollShellSignals) {
    stick_pin(shell);
    // setTimeout(0) 等 Leptos DOM 批处理完成后再 snap，
    // 避免同步 snap 读到旧 scrollHeight 导致的"向上跳动"。
    // 首次 snap 是明确的发送/End 意图，不能被内容增长产生的 Observer 中间态取消。
    Timeout::new(0, move || {
        snap_to_bottom(shell);
        stick_pin(shell);
    })
    .forget();
    // rAF + setTimeout 兜底
    scroll_to_bottom(shell);
}

/// Home 键：关闭跟底并滚到顶。
pub(crate) fn disengage_follow_and_scroll_top(shell: ChatScrollShellSignals) {
    stick_unpin(shell);
    scroll_to_top(shell);
}

/// Markdown/纯文本已实际写入 DOM 后跟底，避免先读旧 `scrollHeight` 再发生内容增高。
pub(crate) fn follow_after_content_paint(shell: ChatScrollShellSignals) {
    snap_to_bottom_if_following(shell);
}

/// 内容根尺寸变化且仍 Pinned 时贴底（ResizeObserver）。
#[inline]
pub(crate) fn on_content_resize_if_pinned(shell: ChatScrollShellSignals) {
    follow_after_content_paint(shell);
}

fn active_session_tail_scroll_fingerprint(list: &[ChatSession], aid: &str) -> u64 {
    let Some(session) = list.iter().find(|s| s.id == aid) else {
        return 0;
    };
    let mut fingerprint = session.messages.len() as u64;
    for msg in session.messages.iter().rev().take(6) {
        fingerprint = fingerprint.wrapping_mul(41);
        fingerprint = fingerprint.wrapping_add(msg.id.len() as u64);
        fingerprint = fingerprint.wrapping_add(msg.text.len() as u64);
        fingerprint = fingerprint.wrapping_add(msg.reasoning_text.len() as u64);
        if let Some(state) = &msg.state {
            fingerprint = fingerprint.wrapping_add(state.to_wire().len() as u64);
        }
        fingerprint = fingerprint.wrapping_add(u64::from(msg.is_tool));
    }
    fingerprint
}

/// 内容信号变化时若仍 Pinned 则 snap 到底。
///
/// stored 尾消息变化和 overlay revision 负责兜底；流式正文以实际 DOM paint 回调为准。
pub(crate) fn wire_content_follow_scroll(chat: ChatSessionSignals, shell: ChatScrollShellSignals) {
    let version = Memo::new(move |_| {
        let aid = chat.active_id.get();
        let fingerprint = chat
            .sessions
            .with(|list| active_session_tail_scroll_fingerprint(list, &aid));
        let rev = chat.stream_overlay_revision.get();
        (fingerprint, rev)
    });
    Effect::new(move |_| {
        let _ = version.get();
        if !shell.auto_scroll_chat.get() {
            return;
        }
        // rAF 等布局完成再读 scrollHeight（Leptos 批处理 + 浏览器布局）
        request_animation_frame(move || {
            snap_to_bottom_if_following(shell);
        });
        // setTimeout 只作失焦兜底；执行时重新检查用户是否已上滚。
        Timeout::new(100, move || {
            snap_to_bottom_if_following(shell);
        })
        .forget();
    });
}
