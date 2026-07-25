//! 消息列滚动壳：统一信号、prepend 锚点、StickToBottom **用户意图**侧。
//!
//! # 跟底状态机（`auto_scroll_chat` ≡ pin）
//!
//! ```text
//!                    wheel↑ / pointer 离底 / Home / 查找跳转
//!     Pinned  ─────────────────────────────────────────────►  Unpinned
//!                    scroll gap ≤ NEAR / 哨兵可见 / 发送 / End
//!     Unpinned ◄─────────────────────────────────────────────  Pinned
//! ```
//!
//! 内容增高时的 snap（ResizeObserver / 信号 Effect / paint 回调）见 [`super::scroll_follow`]。
//! **Unpin 只来自用户意图**（向上滚轮 / 指针拖离底 / Home / 查找跳转）；末尾哨兵 IntersectionObserver **仅** re-pin。
//! **不再**用 `scrollTop` 下降判上滑（内容重写夹低 scrollTop 会误 unpin）。

use leptos::html::Div;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;

use crate::app::app_signals::ChatComposerSignals;

/// 视为「在底部」、可 re-pin 的最大 gap（px）。
pub(crate) const STICK_NEAR_BOTTOM_GAP_PX: i32 = 4;
/// 指针拖拽离底超过此 gap 则 unpin（避免内容增高误触发）。
pub(crate) const STICK_UNPIN_GAP_PX: i32 = 24;

/// 消息列滚动容器与跟底相关信号（`Copy`，供 `column` / `scroll_follow` 共用）。
#[derive(Clone, Copy)]
pub(crate) struct ChatScrollShellSignals {
    pub messages_scroller: NodeRef<Div>,
    /// `true` = StickToBottom **Pinned**。
    pub auto_scroll_chat: RwSignal<bool>,
    pub pointer_scroll_active: RwSignal<bool>,
}

impl ChatScrollShellSignals {
    #[must_use]
    pub fn from_composer(cc: &ChatComposerSignals) -> Self {
        Self {
            messages_scroller: cc.messages_scroller,
            auto_scroll_chat: cc.auto_scroll_chat,
            pointer_scroll_active: cc.messages_pointer_scroll_active,
        }
    }

    /// prepend 更早历史前捕获滚动位置（用于 [`compensate_after_prepend`]）。
    #[must_use]
    pub fn capture_prepend_snapshot(self) -> PrependScrollSnapshot {
        let (scroll_top_before, scroll_height_before) = self
            .messages_scroller
            .get_untracked()
            .map(|el| (el.scroll_top(), el.scroll_height()))
            .unwrap_or((0, 0));
        PrependScrollSnapshot {
            scroll_top_before,
            scroll_height_before,
        }
    }

    /// prepend 更早历史后保持视口锚点（避免列表跳动）。
    pub fn compensate_after_prepend(self, snap: PrependScrollSnapshot) {
        request_animation_frame(move || {
            if let Some(el) = self.messages_scroller.get() {
                let delta = el.scroll_height().saturating_sub(snap.scroll_height_before);
                el.set_scroll_top(snap.scroll_top_before.saturating_add(delta));
            }
        });
    }
}

/// prepend 前一帧的 `scrollTop` / `scrollHeight`。
#[derive(Clone, Copy)]
pub(crate) struct PrependScrollSnapshot {
    pub scroll_top_before: i32,
    pub scroll_height_before: i32,
}

#[inline]
pub(crate) fn scroll_gap_px(scroll_height: i32, scroll_top: i32, client_height: i32) -> i32 {
    scroll_height - scroll_top - client_height
}

#[inline]
pub(crate) fn is_near_bottom(gap_px: i32) -> bool {
    gap_px <= STICK_NEAR_BOTTOM_GAP_PX
}

#[inline]
pub(crate) fn stick_pin(shell: ChatScrollShellSignals) {
    shell.auto_scroll_chat.set(true);
}

#[inline]
pub(crate) fn stick_unpin(shell: ChatScrollShellSignals) {
    shell.auto_scroll_chat.set(false);
}

/// 内容根（ResizeObserver）：优先 `.chat-thread`（含 transcript 与其外层增高），否则 transcript。
pub(crate) fn stick_content_root(scroller: &web_sys::Element) -> Option<web_sys::Element> {
    scroller
        .query_selector(".chat-thread")
        .ok()
        .flatten()
        .or_else(|| {
            scroller
                .query_selector(".chat-tui-transcript")
                .ok()
                .flatten()
        })
}

/// 向上滚轮 → unpin（同步，避免仅靠 scroll 异步判定漏关）。
pub(crate) fn on_messages_wheel_follow_intent(
    shell: ChatScrollShellSignals,
    ev: web_sys::WheelEvent,
) {
    if ev.delta_y() < 0.0 {
        stick_unpin(shell);
    }
}

pub(crate) fn on_messages_pointer_scroll_intent(
    pointer_scroll_active: RwSignal<bool>,
    active: bool,
) {
    pointer_scroll_active.set(active);
}

/// scroll：近底 re-pin；指针拖离底 → unpin。
///
/// **不上滑 unpin**：滚轮上滑由 [`on_messages_wheel_follow_intent`]；拖滚动条依赖
/// `pointerdown` + gap。不再用 `scrollTop` 下降判上滑（工具/DOM 重写会误关跟底）。
pub(crate) fn on_messages_stick_scroll_event(
    shell: ChatScrollShellSignals,
    last_scroll_top: RwSignal<i32>,
) {
    let Some(element) = shell.messages_scroller.get_untracked() else {
        return;
    };
    let top = element.scroll_top();
    let prev_top = last_scroll_top.get_untracked();
    last_scroll_top.set(top);
    let gap = scroll_gap_px(element.scroll_height(), top, element.client_height());
    let pointer_active = shell.pointer_scroll_active.get_untracked();

    if pointer_active && gap > STICK_UNPIN_GAP_PX {
        stick_unpin(shell);
        return;
    }
    // 近底或主动下滑回到阈值内 → re-pin（流式增高时 gap 可能短暂 > NEAR）。
    let scrolled_down = top > prev_top + 2;
    if !pointer_active && (is_near_bottom(gap) || (scrolled_down && gap <= STICK_UNPIN_GAP_PX)) {
        stick_pin(shell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_and_near_bottom() {
        assert_eq!(scroll_gap_px(1000, 800, 200), 0);
        assert_eq!(scroll_gap_px(1000, 700, 200), 100);
        assert!(is_near_bottom(0));
        assert!(is_near_bottom(STICK_NEAR_BOTTOM_GAP_PX));
        assert!(!is_near_bottom(STICK_NEAR_BOTTOM_GAP_PX + 1));
    }
}
