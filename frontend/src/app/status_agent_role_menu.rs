//! 底栏 Agent 角色选择（自定义上拉菜单，替代原生 `<select>`）。
//!
//! Tauri / WebKit 在窗口底栏处渲染原生 `<select>` 下拉时易被裁剪；菜单经 [`Portal`] 挂到
//! `document.body`，并用 **`position: fixed`** 锚定触发按钮，避免 `.shell-main { overflow: hidden }`
//! 与 `.status-chips { overflow-y: hidden }` 裁切。

use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::app::status_tasks_state::StatusTasksSignals;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};

#[derive(Clone, Copy)]
pub struct AgentRoleMenuProps {
    pub st: StatusTasksSignals,
    pub locale: RwSignal<Locale>,
    pub chat: ChatSessionSignals,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub agent_role_user_override: RwSignal<bool>,
    pub menu_open: RwSignal<bool>,
}

#[derive(Clone, Copy)]
struct AgentRoleMenuPortalProps {
    st: StatusTasksSignals,
    locale: RwSignal<Locale>,
    chat: ChatSessionSignals,
    selected_agent_role: RwSignal<Option<String>>,
    agent_role_user_override: RwSignal<bool>,
    menu_open: RwSignal<bool>,
    menu_fixed_style: RwSignal<Option<String>>,
}

fn apply_agent_role_selection(
    chat: ChatSessionSignals,
    selected_agent_role: RwSignal<Option<String>>,
    agent_role_user_override: RwSignal<bool>,
    role_id: Option<String>,
) {
    selected_agent_role.set(role_id);
    agent_role_user_override.set(true);
    chat.clear_stream_resume_handles();
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

fn close_role_menu(menu_open: RwSignal<bool>, menu_fixed_style: RwSignal<Option<String>>) {
    menu_open.set(false);
    menu_fixed_style.set(None);
}

fn role_trigger_label(
    st: StatusTasksSignals,
    locale: Locale,
    selected_agent_role: Option<String>,
) -> String {
    match selected_agent_role {
        Some(id) => id,
        None => {
            let default_id = st
                .status_data
                .get_untracked()
                .and_then(|d| d.default_agent_role_id.clone());
            i18n::status_default_option(locale, default_id.as_deref())
        }
    }
}

fn toggle_role_menu_on_click(
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
fn StatusAgentRoleMenuPortal(props: AgentRoleMenuPortalProps) -> impl IntoView {
    let AgentRoleMenuPortalProps {
        st,
        locale,
        chat,
        selected_agent_role,
        agent_role_user_override,
        menu_open,
        menu_fixed_style,
    } = props;

    view! {
        <Portal>
            <button
                type="button"
                class="status-agent-role-backdrop status-agent-role-backdrop--portal"
                tabindex="-1"
                aria-hidden="true"
                on:click=move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    close_role_menu(menu_open, menu_fixed_style);
                }
            />
            <div
                class="status-agent-role-menu status-agent-role-menu--fixed status-agent-role-menu--portal"
                role="menu"
                prop:style=move || menu_fixed_style.get().unwrap_or_default()
                prop:aria-label=move || i18n::status_role_title_attr(locale.get())
            >
                <button
                    type="button"
                    class="status-agent-role-menu-item"
                    role="menuitem"
                    class:active=move || selected_agent_role.get().is_none()
                    on:click=move |_| {
                        apply_agent_role_selection(
                            chat,
                            selected_agent_role,
                            agent_role_user_override,
                            None,
                        );
                        close_role_menu(menu_open, menu_fixed_style);
                    }
                >
                    {move || {
                        let loc = locale.get();
                        match st
                            .status_data
                            .get()
                            .and_then(|d| d.default_agent_role_id.clone())
                        {
                            Some(id) => i18n::status_default_option(loc, Some(id.as_str())),
                            None => i18n::status_default_option(loc, None),
                        }
                    }}
                </button>
                {move || {
                    st.status_data
                        .get()
                        .map(|d| d.agent_role_ids)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|id| {
                            let id_pick = id.clone();
                            view! {
                                <button
                                    type="button"
                                    class="status-agent-role-menu-item"
                                    role="menuitem"
                                    class:active=move || {
                                        selected_agent_role
                                            .get()
                                            .as_deref()
                                            == Some(id.as_str())
                                    }
                                    on:click=move |_| {
                                        apply_agent_role_selection(
                                            chat,
                                            selected_agent_role,
                                            agent_role_user_override,
                                            Some(id_pick.clone()),
                                        );
                                        close_role_menu(menu_open, menu_fixed_style);
                                    }
                                    >
                                    {id.clone()}
                                </button>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </Portal>
    }
}

#[component]
pub fn StatusAgentRoleMenu(props: AgentRoleMenuProps) -> impl IntoView {
    let AgentRoleMenuProps {
        st,
        locale,
        chat,
        selected_agent_role,
        agent_role_user_override,
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
        <div class="status-agent-role-wrap">
            <button
                type="button"
                class="status-agent-select status-agent-role-trigger"
                class:status-agent-role-trigger-open=move || menu_open.get()
                data-testid="status-agent-role-trigger"
                node_ref=trigger_ref
                prop:title=move || i18n::status_role_title_attr(locale.get())
                prop:aria-expanded=move || menu_open.get()
                aria-haspopup="menu"
                on:click=move |ev: web_sys::MouseEvent| {
                    toggle_role_menu_on_click(ev, menu_open, menu_fixed_style, trigger_ref);
                }
            >
                <span class="status-agent-role-trigger-label">{move || {
                    role_trigger_label(st, locale.get(), selected_agent_role.get())
                }}</span>
                <svg
                    class="status-agent-role-chevron"
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
                <StatusAgentRoleMenuPortal props=AgentRoleMenuPortalProps {
                    st,
                    locale,
                    chat,
                    selected_agent_role,
                    agent_role_user_override,
                    menu_open,
                    menu_fixed_style,
                } />
            </Show>
        </div>
    }
}
