//! 底栏 Agent 角色选择（自定义上拉菜单，替代原生 `<select>`）。
//!
//! Tauri / WebKit 在窗口底栏处渲染原生 `<select>` 下拉时易被裁剪；上拉菜单与侧栏「视图」下拉同源。

use leptos::prelude::*;

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

    view! {
        <div class="status-agent-role-wrap">
            <Show when=move || menu_open.get()>
                <button
                    type="button"
                    class="status-agent-role-backdrop"
                    tabindex="-1"
                    aria-hidden="true"
                    on:click=move |_| menu_open.set(false)
                />
            </Show>
            <button
                type="button"
                class="status-agent-select status-agent-role-trigger"
                class:status-agent-role-trigger-open=move || menu_open.get()
                data-testid="status-agent-role-trigger"
                prop:title=move || i18n::status_role_title_attr(locale.get())
                prop:aria-expanded=move || menu_open.get()
                aria-haspopup="menu"
                on:click=move |_| menu_open.update(|o| *o = !*o)
            >
                <span class="status-agent-role-trigger-label">{move || {
                    let loc = locale.get();
                    match selected_agent_role.get() {
                        Some(id) => id,
                        None => {
                            let default_id = st
                                .status_data
                                .get()
                                .and_then(|d| d.default_agent_role_id.clone());
                            i18n::status_default_option(loc, default_id.as_deref())
                        }
                    }
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
                <div
                    class="status-agent-role-menu"
                    role="menu"
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
                            menu_open.set(false);
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
                                            menu_open.set(false);
                                        }
                                        >
                                        {id.clone()}
                                    </button>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </Show>
        </div>
    }
}
