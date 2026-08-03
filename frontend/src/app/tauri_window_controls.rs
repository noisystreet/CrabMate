//! Tauri 桌面：无边框时提供最小化 / 最大化 / 关闭。
//!
//! 三个窗口操作合并为一个触发按钮，点击弹出下拉子菜单（最小化 / 最大化或还原 / 关闭），
//! 避免在顶栏右侧并排占用三个按钮宽度。菜单经 [`Portal`] 挂到 `document.body` 并用
//! `position: fixed` 锚定触发按钮，避免被 `.shell-topbar` / `.shell-main` 的 overflow 裁切。

use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::i18n::{self, Locale};
use crate::tauri_shell::{
    tauri_main_window_close, tauri_main_window_minimize, tauri_main_window_toggle_maximize,
    tauri_shell_available,
};

fn menu_fixed_style_for_trigger(trigger: &web_sys::HtmlElement) -> String {
    let el = trigger.unchecked_ref::<web_sys::Element>();
    let rect = el.get_bounding_client_rect();
    let viewport_w = web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let right = (viewport_w - rect.right() + 4.0).max(0.0);
    format!(
        "position:fixed;right:{}px;top:{}px;min-width:{}px;left:auto;z-index:201;",
        right,
        rect.bottom(),
        rect.width()
    )
}

fn sync_menu_anchor(
    trigger_ref: NodeRef<html::Button>,
    menu_fixed_style: RwSignal<Option<String>>,
) {
    let Some(trigger) = trigger_ref.get() else {
        return;
    };
    let el: web_sys::HtmlElement = trigger.unchecked_into();
    menu_fixed_style.set(Some(menu_fixed_style_for_trigger(&el)));
}

fn sync_menu_anchor_from_event(
    ev: &web_sys::MouseEvent,
    menu_fixed_style: RwSignal<Option<String>>,
) {
    let Some(target) = ev.current_target() else {
        return;
    };
    let Ok(el) = target.dyn_into::<web_sys::HtmlElement>() else {
        return;
    };
    menu_fixed_style.set(Some(menu_fixed_style_for_trigger(&el)));
}

fn close_menu(menu_open: RwSignal<bool>, menu_fixed_style: RwSignal<Option<String>>) {
    menu_open.set(false);
    menu_fixed_style.set(None);
}

fn toggle_menu_on_click(
    ev: web_sys::MouseEvent,
    menu_open: RwSignal<bool>,
    menu_fixed_style: RwSignal<Option<String>>,
    trigger_ref: NodeRef<html::Button>,
) {
    ev.stop_propagation();
    let next = !menu_open.get_untracked();
    if next {
        sync_menu_anchor_from_event(&ev, menu_fixed_style);
        sync_menu_anchor(trigger_ref, menu_fixed_style);
    } else {
        menu_fixed_style.set(None);
    }
    menu_open.set(next);
}

#[component]
fn TauriWindowControlsMenu(
    locale: RwSignal<Locale>,
    menu_open: RwSignal<bool>,
    menu_fixed_style: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <Portal>
            <button
                type="button"
                class="tauri-win-ctrl-backdrop tauri-win-ctrl-backdrop--portal"
                tabindex="-1"
                aria-hidden="true"
                on:click=move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    close_menu(menu_open, menu_fixed_style);
                }
            />
            <div
                class="tauri-win-ctrl-menu tauri-win-ctrl-menu--fixed tauri-win-ctrl-menu--portal"
                role="menu"
                prop:style=move || menu_fixed_style.get().unwrap_or_default()
                prop:aria-label=move || i18n::ide_tauri_window_controls_aria(locale.get())
            >
                <button
                    type="button"
                    class="tauri-win-ctrl-menu-item"
                    role="menuitem"
                    on:click=move |_| {
                        tauri_main_window_minimize();
                        close_menu(menu_open, menu_fixed_style);
                    }
                >
                    <span class="tauri-win-ctrl-menu-glyph" aria-hidden="true">"−"</span>
                    {move || i18n::ide_tauri_window_minimize(locale.get())}
                </button>
                <button
                    type="button"
                    class="tauri-win-ctrl-menu-item"
                    role="menuitem"
                    on:click=move |_| {
                        tauri_main_window_toggle_maximize();
                        close_menu(menu_open, menu_fixed_style);
                    }
                >
                    <span class="tauri-win-ctrl-menu-glyph" aria-hidden="true">"□"</span>
                    {move || i18n::ide_tauri_window_toggle_maximize(locale.get())}
                </button>
                <button
                    type="button"
                    class="tauri-win-ctrl-menu-item tauri-win-ctrl-menu-item--close"
                    role="menuitem"
                    on:click=move |_| {
                        tauri_main_window_close();
                        close_menu(menu_open, menu_fixed_style);
                    }
                >
                    <span class="tauri-win-ctrl-menu-glyph" aria-hidden="true">"×"</span>
                    {move || i18n::ide_tauri_window_close(locale.get())}
                </button>
            </div>
        </Portal>
    }
}

#[component]
pub fn TauriWindowControls(locale: RwSignal<Locale>) -> impl IntoView {
    let menu_open = RwSignal::new(false);
    let menu_fixed_style = RwSignal::<Option<String>>::new(None);
    let trigger_ref = NodeRef::<html::Button>::new();

    Effect::new(move |_| {
        if !menu_open.get() {
            return;
        }
        sync_menu_anchor(trigger_ref, menu_fixed_style);
        request_animation_frame(move || {
            sync_menu_anchor(trigger_ref, menu_fixed_style);
        });
    });

    view! {
        <Show when=move || tauri_shell_available()>
            <div
                class="tauri-window-controls"
                role="group"
                data-testid="tauri-window-controls"
                prop:aria-label=move || i18n::ide_tauri_window_controls_aria(locale.get())
            >
                <button
                    type="button"
                    class="tauri-win-ctrl tauri-win-ctrl-trigger"
                    class:tauri-win-ctrl-trigger-open=move || menu_open.get()
                    node_ref=trigger_ref
                    prop:title=move || i18n::ide_tauri_window_controls_aria(locale.get())
                    prop:aria-expanded=move || menu_open.get()
                    aria-haspopup="menu"
                    on:click=move |ev: web_sys::MouseEvent| {
                        toggle_menu_on_click(ev, menu_open, menu_fixed_style, trigger_ref);
                    }
                >
                    <span class="tauri-win-ctrl-glyph" aria-hidden="true">"•••"</span>
                </button>
                <Show when=move || menu_open.get()>
                    <TauriWindowControlsMenu
                        locale=locale
                        menu_open=menu_open
                        menu_fixed_style=menu_fixed_style
                    />
                </Show>
            </div>
        </Show>
    }
}
