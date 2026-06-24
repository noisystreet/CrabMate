//! 主聊天区滚动：侧栏「在消息中打开」后滚入视图。
//!
//! 跟底规则见 [`super::scroll_follow`]；滚动壳信号见 [`super::scroll_shell`]。

use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::session_search::scroll_message_into_view;

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
