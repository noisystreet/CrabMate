//! 聊天主列：消息区 Home/End 键盘滚动（从 `chat_column_view` 拆出以降低圈复杂度）。

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::app::scroll_guard::MessagesScrollFromEffectGuard;
use crate::session_ops::messages_scroller_has_non_collapsed_selection;

/// Home/End 所需滚动与自动跟底信号（`Copy`，可安全捕获进闭包）。
#[derive(Clone, Copy)]
pub(crate) struct ChatColumnHomeEndNav {
    pub messages_scroller: NodeRef<leptos::html::Div>,
    pub messages_scroll_from_effect: RwSignal<bool>,
    pub auto_scroll_chat: RwSignal<bool>,
}

fn home_end_ignore_for_form_like_target(he: &web_sys::HtmlElement) -> bool {
    let tag = he.tag_name();
    if tag.eq_ignore_ascii_case("TEXTAREA")
        || tag.eq_ignore_ascii_case("INPUT")
        || tag.eq_ignore_ascii_case("SELECT")
        || tag.eq_ignore_ascii_case("OPTION")
    {
        return true;
    }
    he.is_content_editable()
}

fn spawn_tri_pulse_scroll_top(
    mref: NodeRef<leptos::html::Div>,
    scroll_from_effect: RwSignal<bool>,
) {
    spawn_local(async move {
        let _guard = MessagesScrollFromEffectGuard::new(scroll_from_effect);
        TimeoutFuture::new(0).await;
        if let Some(el) = mref.get() {
            el.set_scroll_top(0);
        }
        TimeoutFuture::new(0).await;
        if let Some(el) = mref.get() {
            el.set_scroll_top(0);
        }
        TimeoutFuture::new(16).await;
        if let Some(el) = mref.get() {
            el.set_scroll_top(0);
        }
    });
}

fn spawn_tri_pulse_scroll_bottom(
    mref: NodeRef<leptos::html::Div>,
    scroll_from_effect: RwSignal<bool>,
) {
    spawn_local(async move {
        let _guard = MessagesScrollFromEffectGuard::new(scroll_from_effect);
        TimeoutFuture::new(0).await;
        if let Some(el) = mref.get() {
            el.set_scroll_top(el.scroll_height());
        }
        TimeoutFuture::new(0).await;
        if let Some(el) = mref.get() {
            el.set_scroll_top(el.scroll_height());
        }
        TimeoutFuture::new(16).await;
        if let Some(el) = mref.get() {
            el.set_scroll_top(el.scroll_height());
        }
    });
}

impl ChatColumnHomeEndNav {
    pub fn keydown_handler(self) -> impl Fn(web_sys::KeyboardEvent) + Clone + 'static {
        move |ev: web_sys::KeyboardEvent| {
            let key = ev.key();
            if key != "End" && key != "Home" {
                return;
            }
            let Some(t) = ev.target() else {
                return;
            };
            let Ok(he) = t.dyn_into::<web_sys::HtmlElement>() else {
                return;
            };
            if home_end_ignore_for_form_like_target(&he) {
                return;
            }
            let mref = self.messages_scroller;
            if let Some(el) = mref.get()
                && messages_scroller_has_non_collapsed_selection(&el)
            {
                return;
            }
            ev.prevent_default();
            let scroll_from_effect = self.messages_scroll_from_effect;
            if key == "Home" {
                self.auto_scroll_chat.set(false);
                spawn_tri_pulse_scroll_top(mref, scroll_from_effect);
                return;
            }
            self.auto_scroll_chat.set(true);
            spawn_tri_pulse_scroll_bottom(mref, scroll_from_effect);
        }
    }
}
