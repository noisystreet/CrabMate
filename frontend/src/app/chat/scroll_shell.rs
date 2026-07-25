//! 消息列滚动壳：统一信号、prepend 后锚点补偿。
//!
//! # 滚动跟底架构
//!
//! - **用户滚动检测**：向上滚轮，或按住指针拖动滚动区域离底时关闭跟底
//! - **自动跟底**：`IntersectionObserver` 只在末尾哨兵可见时恢复跟底，不因内容增长关闭
//! - **流式追底**：内容变化时 `Effect` → rAF → `scrollTop = scrollHeight`（无条件 snap）
//! - **主动滚底**：发送消息 / End 键 → `engage_follow_and_scroll_bottom`

use leptos::html::Div;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;

use crate::app::app_signals::ChatComposerSignals;

/// 消息列滚动容器与跟底相关信号（`Copy`，供 `column` / `scroll_follow` 共用）。
#[derive(Clone, Copy)]
pub(crate) struct ChatScrollShellSignals {
    pub messages_scroller: NodeRef<Div>,
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

/// **入口 A（滚轮）**：向上滚则立即关闭跟底（Observe 异步，此同步关闭防抖）。
pub(crate) fn on_messages_wheel_follow_intent(
    auto_scroll_chat: RwSignal<bool>,
    ev: web_sys::WheelEvent,
) {
    if ev.delta_y() < 0.0 {
        auto_scroll_chat.set(false);
    }
}

pub(crate) fn on_messages_pointer_scroll_intent(
    pointer_scroll_active: RwSignal<bool>,
    active: bool,
) {
    pointer_scroll_active.set(active);
}

/// 仅在指针按住期间把离底滚动认作用户意图，避免内容增长产生的 `scroll` 事件误关跟底。
pub(crate) fn on_messages_pointer_scroll_event(shell: ChatScrollShellSignals, ev: web_sys::Event) {
    use wasm_bindgen::JsCast;

    if !shell.pointer_scroll_active.get_untracked() {
        return;
    }
    let Some(target) = ev.target() else {
        return;
    };
    let Ok(element) = target.dyn_into::<web_sys::HtmlElement>() else {
        return;
    };
    let gap = element.scroll_height() - element.scroll_top() - element.client_height();
    if gap > 24 {
        shell.auto_scroll_chat.set(false);
    }
}
