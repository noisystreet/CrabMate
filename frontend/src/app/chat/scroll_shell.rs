//! 消息列滚动壳：统一信号、用户滚动意图（入口 A）、prepend 后锚点补偿。

use leptos::html::Div;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::app::app_signals::ChatComposerSignals;
use crate::app_prefs::AUTO_SCROLL_RESUME_GAP_PX;

/// 消息列滚动容器与跟底相关信号（`Copy`，供 `column` / `scroll_follow` / 加载更早共用）。
#[derive(Clone, Copy)]
pub(crate) struct ChatScrollShellSignals {
    pub messages_scroller: NodeRef<Div>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub messages_scroll_from_effect: RwSignal<bool>,
    pub last_messages_scroll_top: RwSignal<i32>,
}

impl ChatScrollShellSignals {
    #[must_use]
    pub fn from_composer(cc: &ChatComposerSignals) -> Self {
        Self {
            messages_scroller: cc.messages_scroller,
            auto_scroll_chat: cc.auto_scroll_chat,
            messages_scroll_from_effect: cc.messages_scroll_from_effect,
            last_messages_scroll_top: cc.last_messages_scroll_top,
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
        self.messages_scroll_from_effect.set(true);
        request_animation_frame(move || {
            if let Some(el) = self.messages_scroller.get() {
                let delta = el.scroll_height().saturating_sub(snap.scroll_height_before);
                el.set_scroll_top(snap.scroll_top_before.saturating_add(delta));
            }
            self.messages_scroll_from_effect.set(false);
        });
    }
}

/// prepend 前一帧的 `scrollTop` / `scrollHeight`。
#[derive(Clone, Copy)]
pub(crate) struct PrependScrollSnapshot {
    pub scroll_top_before: i32,
    pub scroll_height_before: i32,
}

/// **入口 A（滚轮）**：向上滚则关闭跟底。
pub(crate) fn on_messages_wheel_follow_intent(
    auto_scroll_chat: RwSignal<bool>,
    ev: web_sys::WheelEvent,
) {
    if ev.delta_y() < 0.0 {
        auto_scroll_chat.set(false);
    }
}

/// **入口 A（滚动条）**：距底过远关闭跟底；回到底部附近且向下滚则恢复跟底。
pub(crate) fn on_messages_scroll_follow_intent(
    shell: ChatScrollShellSignals,
    el: &web_sys::HtmlElement,
) {
    if shell.messages_scroll_from_effect.get_untracked() {
        return;
    }
    let top = el.scroll_top();
    let prev_top = shell.last_messages_scroll_top.get_untracked();
    shell.last_messages_scroll_top.set(top);
    let gap = el.scroll_height() - top - el.client_height();
    if gap > AUTO_SCROLL_RESUME_GAP_PX {
        shell.auto_scroll_chat.set(false);
    } else if !shell.auto_scroll_chat.get_untracked() && top >= prev_top {
        shell.auto_scroll_chat.set(true);
    }
}

/// 将 `on:scroll` 事件委托给 [`on_messages_scroll_follow_intent`]。
pub(crate) fn on_messages_scroll_event(shell: ChatScrollShellSignals, ev: web_sys::Event) {
    if let Some(t) = ev.target()
        && let Ok(el) = t.dyn_into::<web_sys::HtmlElement>()
    {
        on_messages_scroll_follow_intent(shell, &el);
    }
}
