//! 聊天主列：消息区 Home/End 键盘滚动（从 `chat_column_view` 拆出以降低圈复杂度）。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::app::chat::scroll_follow::{
    disengage_follow_and_scroll_top, engage_follow_and_scroll_bottom,
};
use crate::app::chat::scroll_shell::ChatScrollShellSignals;
use crate::session_ops::messages_scroller_has_non_collapsed_selection;

/// Home/End 所需滚动与自动跟底信号（`Copy`，可安全捕获进闭包）。
pub(crate) type ChatColumnHomeEndNav = ChatScrollShellSignals;

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
            if let Some(el) = self.messages_scroller.get()
                && messages_scroller_has_non_collapsed_selection(&el)
            {
                return;
            }
            ev.prevent_default();
            if key == "Home" {
                disengage_follow_and_scroll_top(self);
                return;
            }
            engage_follow_and_scroll_bottom(self);
        }
    }
}
