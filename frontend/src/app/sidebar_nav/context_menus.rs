use leptos::prelude::*;

use crate::i18n;
use crate::session_ops::{
    SessionContextAnchor, delete_session_after_confirm, export_session_json_for_id,
    export_session_markdown_for_id, set_session_pinned, set_session_starred,
};

use crate::app::shell_runtime_context::expect_chat_shell_ctx;

#[component]
pub(super) fn SessionContextMenuLayer(
    session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    session_modal: RwSignal<bool>,
    mobile_nav_open: RwSignal<bool>,
) -> impl IntoView {
    let shell = expect_chat_shell_ctx();
    let chat = shell.chat;
    let draft = shell.composer.draft;
    let locale = shell.locale;
    let apply_assistant_display_filters = shell.apply_assistant_display_filters;
    view! {
        <Show when=move || session_context_menu.get().is_some()>
            <div class="session-ctx-layer">
                <div
                    class="session-ctx-backdrop"
                    aria-hidden="true"
                    on:click=move |_| session_context_menu.set(None)
                ></div>
                <div
                    class="session-ctx-menu"
                    role="menu"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    style=move || {
                        session_context_menu
                            .get()
                            .map(|a| format!("left:{}px;top:{}px;", a.x, a.y))
                            .unwrap_or_default()
                    }
                >
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            session_context_menu.set(None);
                            session_modal.set(true);
                            mobile_nav_open.set(false);
                        }
                    >
                        {move || i18n::nav_manage_sessions(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(a) = session_context_menu.get() else {
                                return;
                            };
                            let id = a.session_id.clone();
                            let starred = chat.sessions.with(|list| {
                                list.iter()
                                    .find(|s| s.id == id)
                                    .map(|s| s.starred)
                                    .unwrap_or(false)
                            });
                            session_context_menu.set(None);
                            set_session_starred(chat.sessions, &id, !starred);
                        }
                    >
                        {move || {
                            let _ = chat.sessions.get();
                            let loc = locale.get();
                            let Some(a) = session_context_menu.get() else {
                                return i18n::ctx_star_session(loc).to_string();
                            };
                            let starred = chat.sessions.with(|list| {
                                list.iter()
                                    .find(|s| s.id == a.session_id)
                                    .map(|s| s.starred)
                                    .unwrap_or(false)
                            });
                            if starred {
                                i18n::ctx_unstar_session(loc).to_string()
                            } else {
                                i18n::ctx_star_session(loc).to_string()
                            }
                        }}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(a) = session_context_menu.get() else {
                                return;
                            };
                            let id = a.session_id.clone();
                            let pinned = chat.sessions.with(|list| {
                                list.iter()
                                    .find(|s| s.id == id)
                                    .map(|s| s.pinned)
                                    .unwrap_or(false)
                            });
                            session_context_menu.set(None);
                            set_session_pinned(chat.sessions, &id, !pinned);
                        }
                    >
                        {move || {
                            let _ = chat.sessions.get();
                            let loc = locale.get();
                            let Some(a) = session_context_menu.get() else {
                                return i18n::ctx_pin_session(loc).to_string();
                            };
                            let pinned = chat.sessions.with(|list| {
                                list.iter()
                                    .find(|s| s.id == a.session_id)
                                    .map(|s| s.pinned)
                                    .unwrap_or(false)
                            });
                            if pinned {
                                i18n::ctx_unpin_session(loc).to_string()
                            } else {
                                i18n::ctx_pin_session(loc).to_string()
                            }
                        }}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(a) = session_context_menu.get() else {
                                return;
                            };
                            let id = a.session_id;
                            session_context_menu.set(None);
                            export_session_json_for_id(
                                chat.sessions,
                                &id,
                                locale.get_untracked(),
                                apply_assistant_display_filters.get_untracked(),
                            );
                        }
                    >
                        {move || i18n::ctx_export_json(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(a) = session_context_menu.get() else {
                                return;
                            };
                            let id = a.session_id;
                            session_context_menu.set(None);
                            export_session_markdown_for_id(
                                chat.sessions,
                                &id,
                                locale.get_untracked(),
                                apply_assistant_display_filters.get_untracked(),
                            );
                        }
                    >
                        {move || i18n::ctx_export_md(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item session-ctx-item-danger"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(a) = session_context_menu.get() else {
                                return;
                            };
                            let id = a.session_id;
                            session_context_menu.set(None);
                            delete_session_after_confirm(
                                chat.sessions,
                                chat.active_id,
                                draft,
                                chat.session_sync,
                                &id,
                                locale.get_untracked(),
                            );
                        }
                    >
                        {move || i18n::ctx_delete_session(locale.get())}
                    </button>
                </div>
            </div>
        </Show>
    }
}

#[component]
pub(super) fn RailContextMenuLayer(
    locale: RwSignal<crate::i18n::Locale>,
    sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    session_modal: RwSignal<bool>,
    mobile_nav_open: RwSignal<bool>,
    sidebar_search_panel_open: RwSignal<bool>,
    chat_find_panel_open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <Show when=move || sidebar_rail_ctx_menu.get().is_some()>
            <div class="session-ctx-layer">
                <div
                    class="session-ctx-backdrop"
                    aria-hidden="true"
                    on:click=move |_| sidebar_rail_ctx_menu.set(None)
                ></div>
                <div
                    class="session-ctx-menu"
                    role="menu"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    style=move || {
                        sidebar_rail_ctx_menu
                            .get()
                            .map(|(x, y)| format!("left:{}px;top:{}px;", x, y))
                            .unwrap_or_default()
                    }
                >
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            sidebar_rail_ctx_menu.set(None);
                            session_modal.set(true);
                            mobile_nav_open.set(false);
                        }
                    >
                        {move || i18n::nav_manage_sessions(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            sidebar_rail_ctx_menu.set(None);
                            sidebar_search_panel_open.set(true);
                        }
                    >
                        {move || i18n::nav_rail_ctx_filter_and_search(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            sidebar_rail_ctx_menu.set(None);
                            chat_find_panel_open.set(true);
                        }
                    >
                        {move || i18n::nav_rail_ctx_find_in_chat(locale.get())}
                    </button>
                </div>
            </div>
        </Show>
    }
}
