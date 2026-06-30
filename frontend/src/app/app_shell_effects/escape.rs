//! 全局 **`Escape`**：按固定顺序关闭菜单 / 抽屉 / 模态（处理器内多 **`get_untracked`**，**不**把各面板开关订阅进同一 `Effect` 的依赖图）。

use leptos::prelude::*;
use leptos_dom::helpers::window_event_listener;
use wasm_bindgen::JsCast;

use crate::app::app_signals::IdeChromeSignals;
use crate::ide_confirm::dismiss_ide_confirm;
use crate::session_ops::SessionContextAnchor;

/// 供全局 **`Escape`** 处理器按固定顺序关闭的模态/抽屉句柄。
#[derive(Clone, Copy)]
pub struct ShellEscapeSignals {
    pub session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    pub workspace_context_menu:
        RwSignal<Option<crate::workspace_context_menu::WorkspaceContextAnchor>>,
    pub workspace_pending_create:
        RwSignal<Option<crate::workspace_context_menu::WorkspacePendingCreate>>,
    pub sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    pub chat_find_panel_open: RwSignal<bool>,
    pub ide_chrome: IdeChromeSignals,
    pub sidebar_search_panel_open: RwSignal<bool>,
    pub view_menu_open: RwSignal<bool>,
    pub ide_menubar_dropdown_open: RwSignal<bool>,
    pub mobile_nav_open: RwSignal<bool>,
    pub changelist_modal_open: RwSignal<bool>,
    pub settings_modal: RwSignal<bool>,
    pub ide_settings_page: RwSignal<bool>,
    pub session_modal: RwSignal<bool>,
}

/// 焦点在可编辑控件上时不应触发全局快捷键（与 [`super::session_delete_hotkey`] 共用）。
pub(crate) fn keyboard_event_target_is_text_entry(ev: &web_sys::KeyboardEvent) -> bool {
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

fn dismiss_ide_escape_layers(chrome: IdeChromeSignals) -> bool {
    if chrome.confirm_pending.get_untracked().is_some() {
        dismiss_ide_confirm(chrome.confirm_signals());
        return true;
    }
    if chrome.new_file_modal_open.get_untracked() {
        chrome.new_file_modal_open.set(false);
        chrome.new_file_path_draft.set(String::new());
        return true;
    }
    if chrome.find_panel_open.get_untracked() {
        chrome.find_panel_open.set(false);
        return true;
    }
    if chrome.goto_panel_open.get_untracked() {
        chrome.goto_panel_open.set(false);
        return true;
    }
    false
}

fn dismiss_workspace_escape_layers(shell: ShellEscapeSignals) -> bool {
    if shell.session_context_menu.get_untracked().is_some() {
        shell.session_context_menu.set(None);
        return true;
    }
    if shell.workspace_context_menu.get_untracked().is_some() {
        shell.workspace_context_menu.set(None);
        return true;
    }
    if shell.workspace_pending_create.get_untracked().is_some() {
        shell.workspace_pending_create.set(None);
        return true;
    }
    if shell.sidebar_rail_ctx_menu.get_untracked().is_some() {
        shell.sidebar_rail_ctx_menu.set(None);
        return true;
    }
    false
}

fn dismiss_shell_escape_layers(shell: ShellEscapeSignals) -> bool {
    if shell.chat_find_panel_open.get_untracked() {
        shell.chat_find_panel_open.set(false);
        return true;
    }
    if shell.sidebar_search_panel_open.get_untracked() {
        shell.sidebar_search_panel_open.set(false);
        return true;
    }
    if shell.view_menu_open.get_untracked() {
        shell.view_menu_open.set(false);
        return true;
    }
    if shell.ide_menubar_dropdown_open.get_untracked() {
        shell.ide_menubar_dropdown_open.set(false);
        return true;
    }
    if shell.mobile_nav_open.get_untracked() {
        shell.mobile_nav_open.set(false);
        return true;
    }
    false
}

fn dismiss_modal_escape_layers(shell: ShellEscapeSignals) -> bool {
    if shell.changelist_modal_open.get_untracked() {
        shell.changelist_modal_open.set(false);
        return true;
    }
    if shell.settings_modal.get_untracked() {
        shell.settings_modal.set(false);
        return true;
    }
    if shell.ide_settings_page.get_untracked() {
        shell.ide_settings_page.set(false);
        return true;
    }
    if shell.session_modal.get_untracked() {
        shell.session_modal.set(false);
        return true;
    }
    false
}

fn dismiss_one_escape_layer(shell: ShellEscapeSignals) {
    if dismiss_ide_escape_layers(shell.ide_chrome) {
        return;
    }
    if dismiss_workspace_escape_layers(shell) {
        return;
    }
    if dismiss_shell_escape_layers(shell) {
        return;
    }
    let _ = dismiss_modal_escape_layers(shell);
}

/// 在输入控件外按 **`Escape`** 按层关闭：会话菜单 → 侧栏菜单 → 查找 → … → 会话管理模态。
pub fn wire_escape_key_layered_dismiss(shell: ShellEscapeSignals) {
    Effect::new(move |_| {
        let h = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if ev.key() != "Escape" {
                return;
            }
            if keyboard_event_target_is_text_entry(&ev) {
                return;
            }
            ev.prevent_default();
            dismiss_one_escape_layer(shell);
        });
        on_cleanup(move || h.remove());
    });
}
