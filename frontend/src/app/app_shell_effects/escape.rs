//! 全局 **`Escape`**：按固定顺序关闭菜单 / 抽屉 / 模态（处理器内多 **`get_untracked`**，**不**把各面板开关订阅进同一 `Effect` 的依赖图）。

use leptos::prelude::*;
use leptos_dom::helpers::window_event_listener;
use wasm_bindgen::JsCast;

use crate::session_ops::SessionContextAnchor;

/// 供全局 **`Escape`** 处理器按固定顺序关闭的模态/抽屉句柄。
#[derive(Clone, Copy)]
pub struct ShellEscapeSignals {
    pub session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    pub sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    pub chat_find_panel_open: RwSignal<bool>,
    pub sidebar_search_panel_open: RwSignal<bool>,
    pub view_menu_open: RwSignal<bool>,
    pub mobile_nav_open: RwSignal<bool>,
    pub changelist_modal_open: RwSignal<bool>,
    pub settings_modal: RwSignal<bool>,
    pub session_modal: RwSignal<bool>,
}

fn escape_event_target_is_text_entry(ev: &web_sys::KeyboardEvent) -> bool {
    let Some(t) = ev.target() else {
        return false;
    };
    let Ok(he) = t.dyn_into::<web_sys::HtmlElement>() else {
        return false;
    };
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

fn dismiss_one_escape_layer(shell: ShellEscapeSignals) {
    if shell.session_context_menu.get_untracked().is_some() {
        shell.session_context_menu.set(None);
        return;
    }
    if shell.sidebar_rail_ctx_menu.get_untracked().is_some() {
        shell.sidebar_rail_ctx_menu.set(None);
        return;
    }
    if shell.chat_find_panel_open.get_untracked() {
        shell.chat_find_panel_open.set(false);
        return;
    }
    if shell.sidebar_search_panel_open.get_untracked() {
        shell.sidebar_search_panel_open.set(false);
        return;
    }
    if shell.view_menu_open.get_untracked() {
        shell.view_menu_open.set(false);
        return;
    }
    if shell.mobile_nav_open.get_untracked() {
        shell.mobile_nav_open.set(false);
        return;
    }
    if shell.changelist_modal_open.get_untracked() {
        shell.changelist_modal_open.set(false);
        return;
    }
    if shell.settings_modal.get_untracked() {
        shell.settings_modal.set(false);
        return;
    }
    if shell.session_modal.get_untracked() {
        shell.session_modal.set(false);
    }
}

/// 在输入控件外按 **`Escape`** 按层关闭：会话菜单 → 侧栏菜单 → 查找 → … → 会话管理模态。
pub fn wire_escape_key_layered_dismiss(shell: ShellEscapeSignals) {
    Effect::new(move |_| {
        let h = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if ev.key() != "Escape" {
                return;
            }
            if escape_event_target_is_text_entry(&ev) {
                return;
            }
            ev.prevent_default();
            dismiss_one_escape_layer(shell);
        });
        on_cleanup(move || h.remove());
    });
}
