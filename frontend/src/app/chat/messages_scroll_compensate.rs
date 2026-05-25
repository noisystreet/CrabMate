//! 分页 prepend 更早消息后保持消息列滚动锚点。

use leptos::html::Div;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;

use super::message_virtual_viewport::sync_virtual_scroll_signals_from_element;

/// 拉取更早一页时的滚动容器与补偿快照。
#[derive(Clone, Copy)]
pub(crate) struct LoadOlderScrollContext {
    pub messages_scroller: NodeRef<Div>,
    pub messages_scroll_from_effect: RwSignal<bool>,
    pub virtual_scroll_top: RwSignal<i32>,
    pub virtual_viewport_height: RwSignal<i32>,
    pub scroll_top_before: i32,
    pub scroll_height_before: i32,
}

impl LoadOlderScrollContext {
    #[must_use]
    pub fn capture(
        messages_scroller: NodeRef<Div>,
        messages_scroll_from_effect: RwSignal<bool>,
        virtual_scroll_top: RwSignal<i32>,
        virtual_viewport_height: RwSignal<i32>,
    ) -> Self {
        let (scroll_top_before, scroll_height_before) = messages_scroller
            .get_untracked()
            .map(|el| (el.scroll_top(), el.scroll_height()))
            .unwrap_or((0, 0));
        Self {
            messages_scroller,
            messages_scroll_from_effect,
            virtual_scroll_top,
            virtual_viewport_height,
            scroll_top_before,
            scroll_height_before,
        }
    }
}

/// prepend 更早历史后保持视口锚点（避免列表跳动）。
pub(crate) fn compensate_messages_scroll_after_prepend(ctx: LoadOlderScrollContext) {
    let LoadOlderScrollContext {
        messages_scroller,
        messages_scroll_from_effect,
        virtual_scroll_top,
        virtual_viewport_height,
        scroll_top_before,
        scroll_height_before,
    } = ctx;
    messages_scroll_from_effect.set(true);
    request_animation_frame(move || {
        if let Some(el) = messages_scroller.get() {
            let delta = el.scroll_height().saturating_sub(scroll_height_before);
            el.set_scroll_top(scroll_top_before.saturating_add(delta));
            sync_virtual_scroll_signals_from_element(
                &el,
                virtual_scroll_top,
                virtual_viewport_height,
            );
        }
        messages_scroll_from_effect.set(false);
    });
}
