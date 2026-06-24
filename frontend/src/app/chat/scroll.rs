//! 主聊天区滚动：视口测量、侧栏「在消息中打开」后滚入视图。
//!
//! 跟底规则与主动滚底见 [`super::scroll_follow`]。

use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::session_search::scroll_message_into_view;

/// 测量消息列视口高度（供流式跟底时的尾部虚拟窗口；避免默认 600px 偏差）。
pub(crate) fn wire_messages_virtual_viewport_measure(
    messages_scroller: NodeRef<Div>,
    virtual_viewport_height: RwSignal<i32>,
) {
    Effect::new(move |_| {
        if let Some(el) = messages_scroller.get() {
            let h = el.client_height();
            if h > 0 {
                virtual_viewport_height.set(h);
            }
        }
    });
}

/// 侧栏「在消息中打开」后滚动到对应气泡。
pub(crate) fn wire_focus_message_after_nav(focus_message_id_after_nav: RwSignal<Option<String>>) {
    Effect::new({
        let focus_message_id_after_nav = focus_message_id_after_nav;
        move |_| {
            let Some(mid) = focus_message_id_after_nav.get() else {
                return;
            };
            focus_message_id_after_nav.set(None);
            let mid = mid.clone();
            spawn_local(async move {
                TimeoutFuture::new(48).await;
                scroll_message_into_view(&mid);
                TimeoutFuture::new(120).await;
                scroll_message_into_view(&mid);
            });
        }
    });
}
