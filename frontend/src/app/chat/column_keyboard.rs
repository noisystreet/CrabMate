//! 聊天主列：消息区 Home/End 键盘滚动（从 `chat_column_view` 拆出以降低圈复杂度）。

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::app::chat::message_virtual_viewport::sync_virtual_scroll_signals_from_element;
use crate::app::scroll_guard::MessagesScrollFromEffectGuard;
use crate::session_ops::messages_scroller_has_non_collapsed_selection;

/// Home/End 所需滚动与自动跟底信号（`Copy`，可安全捕获进闭包）。
#[derive(Clone, Copy)]
pub(crate) struct ChatColumnHomeEndNav {
    pub messages_scroller: NodeRef<leptos::html::Div>,
    pub messages_scroll_from_effect: RwSignal<bool>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub virtual_scroll_top: RwSignal<i32>,
    pub virtual_viewport_height: RwSignal<i32>,
}

fn is_chat_composer_textarea(he: &web_sys::HtmlElement) -> bool {
    he.tag_name().eq_ignore_ascii_case("TEXTAREA") && he.class_list().contains("composer-input")
}

/// 其它表单控件上的 Home/End 仍交给浏览器；聊天输入框的 Home/End 用于滚动消息列。
fn home_end_ignore_for_form_like_target(he: &web_sys::HtmlElement) -> bool {
    if is_chat_composer_textarea(he) {
        return false;
    }
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
    virtual_scroll_top: RwSignal<i32>,
    virtual_viewport_height: RwSignal<i32>,
) {
    spawn_local(async move {
        let _guard = MessagesScrollFromEffectGuard::new(scroll_from_effect);
        for delay in [0, 0, 16] {
            TimeoutFuture::new(delay).await;
            if let Some(el) = mref.get() {
                el.set_scroll_top(0);
                sync_virtual_scroll_signals_from_element(
                    &el,
                    virtual_scroll_top,
                    virtual_viewport_height,
                );
            }
        }
    });
}

fn spawn_tri_pulse_scroll_bottom(
    mref: NodeRef<leptos::html::Div>,
    scroll_from_effect: RwSignal<bool>,
    virtual_scroll_top: RwSignal<i32>,
    virtual_viewport_height: RwSignal<i32>,
) {
    spawn_local(async move {
        let _guard = MessagesScrollFromEffectGuard::new(scroll_from_effect);
        for delay in [0, 0, 16] {
            TimeoutFuture::new(delay).await;
            if let Some(el) = mref.get() {
                el.set_scroll_top(el.scroll_height());
                sync_virtual_scroll_signals_from_element(
                    &el,
                    virtual_scroll_top,
                    virtual_viewport_height,
                );
            }
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
            let vtop = self.virtual_scroll_top;
            let vvh = self.virtual_viewport_height;
            if key == "Home" {
                self.auto_scroll_chat.set(false);
                spawn_tri_pulse_scroll_top(mref, scroll_from_effect, vtop, vvh);
                return;
            }
            self.auto_scroll_chat.set(true);
            spawn_tri_pulse_scroll_bottom(mref, scroll_from_effect, vtop, vvh);
        }
    }
}
