//! 底栏 Ask / Plan / Act 模式选择（自定义上拉子菜单，替代三段按钮）。
//!
//! 与 [`StatusAgentRoleMenu`] 同理：Tauri / WebKit 在窗口底栏渲染原生下拉易被裁剪，
//! 菜单经 [`Portal`] 挂到 `document.body`，并用 **`position: fixed`** 锚定触发按钮，
//! 避免 `.shell-main { overflow: hidden }` 与 `.status-chips { overflow-y: hidden }` 裁切。

use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::session_ops::{make_message_id, message_created_ms, patch_active_session};
use crate::storage::StoredMessage;

const MODES: [(&str, &str); 3] = [("ask", "Ask"), ("plan", "Plan"), ("act", "Act")];

#[derive(Clone, Copy)]
pub struct SessionModeSegProps {
    pub locale: RwSignal<Locale>,
    pub chat: ChatSessionSignals,
    pub selected_session_mode: RwSignal<String>,
    pub session_mode_user_override: RwSignal<bool>,
    pub menu_open: RwSignal<bool>,
}

#[derive(Clone, Copy)]
struct SessionModeMenuPortalProps {
    locale: RwSignal<Locale>,
    chat: ChatSessionSignals,
    selected_session_mode: RwSignal<String>,
    session_mode_user_override: RwSignal<bool>,
    menu_open: RwSignal<bool>,
    menu_fixed_style: RwSignal<Option<String>>,
}

fn apply_session_mode_selection(
    chat: ChatSessionSignals,
    selected_session_mode: RwSignal<String>,
    session_mode_user_override: RwSignal<bool>,
    locale: Locale,
    mode: &str,
) {
    let mode = mode.trim().to_ascii_lowercase();
    if !matches!(mode.as_str(), "ask" | "plan" | "act") {
        return;
    }
    if selected_session_mode.get_untracked() == mode {
        session_mode_user_override.set(true);
        return;
    }
    selected_session_mode.set(mode.clone());
    session_mode_user_override.set(true);
    chat.clear_stream_resume_handles();
    let notice = i18n::status_session_mode_switched(locale, mode.as_str());
    let mid = make_message_id();
    let now = message_created_ms();
    patch_active_session(chat.sessions, &chat.active_id.get_untracked(), |s| {
        s.messages.push(StoredMessage {
            id: mid,
            role: "system".into(),
            text: notice,
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: now,
        });
    });
}

fn menu_fixed_style_for_trigger(trigger: &web_sys::HtmlElement) -> String {
    let el = trigger.unchecked_ref::<web_sys::Element>();
    let rect = el.get_bounding_client_rect();
    let viewport_h = web_sys::window()
        .and_then(|w| w.inner_height().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let bottom = (viewport_h - rect.top() + 4.0).max(0.0);
    format!(
        "position:fixed;left:{}px;bottom:{}px;min-width:{}px;top:auto;z-index:201;",
        rect.left(),
        bottom,
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

fn close_mode_menu(menu_open: RwSignal<bool>, menu_fixed_style: RwSignal<Option<String>>) {
    menu_open.set(false);
    menu_fixed_style.set(None);
}

fn mode_trigger_label(locale: Locale, mode: &str) -> String {
    MODES
        .iter()
        .find(|(id, _)| *id == mode)
        .map(|(_, short)| (*short).to_string())
        .unwrap_or_else(|| i18n::status_session_mode_title(locale, mode))
}

fn toggle_mode_menu_on_click(
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
fn StatusSessionModeMenuPortal(props: SessionModeMenuPortalProps) -> impl IntoView {
    let SessionModeMenuPortalProps {
        locale,
        chat,
        selected_session_mode,
        session_mode_user_override,
        menu_open,
        menu_fixed_style,
    } = props;

    view! {
        <Portal>
            <button
                type="button"
                class="status-mode-backdrop status-mode-backdrop--portal"
                tabindex="-1"
                aria-hidden="true"
                on:click=move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    close_mode_menu(menu_open, menu_fixed_style);
                }
            />
            <div
                class="status-mode-menu status-mode-menu--fixed status-mode-menu--portal"
                role="menu"
                prop:style=move || menu_fixed_style.get().unwrap_or_default()
                prop:aria-label=move || i18n::status_mode_label(locale.get())
            >
                {MODES
                    .into_iter()
                    .map(|(id, short)| {
                        let id_owned = id.to_string();
                        let id_for_active = id_owned.clone();
                        let id_for_click = id_owned.clone();
                        view! {
                            <button
                                type="button"
                                class="status-mode-menu-item"
                                role="menuitem"
                                class:active=move || {
                                    selected_session_mode.get() == id_for_active
                                }
                                prop:title=move || {
                                    i18n::status_session_mode_title(locale.get(), id)
                                }
                                on:click=move |_| {
                                    apply_session_mode_selection(
                                        chat,
                                        selected_session_mode,
                                        session_mode_user_override,
                                        locale.get_untracked(),
                                        id_for_click.as_str(),
                                    );
                                    close_mode_menu(menu_open, menu_fixed_style);
                                }
                            >
                                {short}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
        </Portal>
    }
}

#[component]
pub fn StatusSessionModeSeg(props: SessionModeSegProps) -> impl IntoView {
    let SessionModeSegProps {
        locale,
        chat,
        selected_session_mode,
        session_mode_user_override,
        menu_open,
    } = props;

    let trigger_ref = NodeRef::<html::Button>::new();
    let menu_fixed_style = RwSignal::<Option<String>>::new(None);

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
        <div class="status-mode-wrap">
            <button
                type="button"
                class="status-mode-trigger"
                class:status-mode-trigger-open=move || menu_open.get()
                data-testid="status-mode-trigger"
                node_ref=trigger_ref
                prop:title=move || i18n::status_mode_label(locale.get())
                prop:aria-expanded=move || menu_open.get()
                aria-haspopup="menu"
                on:click=move |ev: web_sys::MouseEvent| {
                    toggle_mode_menu_on_click(ev, menu_open, menu_fixed_style, trigger_ref);
                }
            >
                <span class="status-mode-trigger-label">{move || {
                    mode_trigger_label(locale.get(), selected_session_mode.get().as_str())
                }}</span>
                <svg
                    class="status-mode-chevron"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <polyline points="6 9 12 15 18 9" />
                </svg>
            </button>
            <Show when=move || menu_open.get()>
                <StatusSessionModeMenuPortal props=SessionModeMenuPortalProps {
                    locale,
                    chat,
                    selected_session_mode,
                    session_mode_user_override,
                    menu_open,
                    menu_fixed_style,
                } />
            </Show>
        </div>
    }
}
