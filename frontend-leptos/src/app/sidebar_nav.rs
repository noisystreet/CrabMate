//! 左侧导航：品牌、新对话、会话筛选与列表、全文搜索命中、会话右键菜单。
//!
//! 「筛选会话」「搜索消息」输入框仍即时写入 `sidebar_session_query` / `global_message_query`；
//! 列表与 `collect_message_search_hits` 使用防抖后的副本，避免每次 `input` 全量遍历。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;

use crate::debounce_schedule;
use crate::i18n::{self, Locale};
use crate::session_ops::{
    SessionContextAnchor, clamp_session_ctx_menu_pos, delete_session_after_confirm,
    export_session_json_for_id, export_session_markdown_for_id, flush_composer_draft_to_session,
};
use crate::session_search::{
    MESSAGE_SEARCH_MAX_HITS, collect_message_search_hits, normalize_search_query,
    session_title_matches,
};
use crate::storage::ChatSession;

/// 会话标题筛选防抖（毫秒）。
const SIDEBAR_SESSION_FILTER_DEBOUNCE_MS: u32 = 250;
/// 跨会话消息搜索防抖（毫秒）。
const GLOBAL_MESSAGE_SEARCH_DEBOUNCE_MS: u32 = 250;

fn debounce_signal_to_effect(source: RwSignal<String>, target: RwSignal<String>, delay_ms: u32) {
    let debounce_seq: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    Effect::new({
        let debounce_seq = Rc::clone(&debounce_seq);
        move |_| {
            let v = source.get();
            let id = debounce_seq.get().wrapping_add(1);
            debounce_seq.set(id);
            let seq = Rc::clone(&debounce_seq);
            spawn_local(async move {
                TimeoutFuture::new(delay_ms).await;
                if debounce_schedule::debounce_should_apply(id, seq.get()) {
                    target.set(v);
                }
            });
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub fn sidebar_nav_view(
    locale: RwSignal<Locale>,
    mobile_nav_open: RwSignal<bool>,
    session_modal: RwSignal<bool>,
    new_session: impl Fn() + Clone + 'static,
    sidebar_session_query: RwSignal<String>,
    global_message_query: RwSignal<String>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    conversation_id: RwSignal<Option<String>>,
    conversation_revision: RwSignal<Option<u64>>,
    focus_message_id_after_nav: RwSignal<Option<String>>,
    session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    composer_buf_nav: Arc<Mutex<String>>,
) -> impl IntoView {
    let sidebar_filter_debounced = RwSignal::new(String::new());
    let global_message_filter_debounced = RwSignal::new(String::new());
    debounce_signal_to_effect(
        sidebar_session_query,
        sidebar_filter_debounced,
        SIDEBAR_SESSION_FILTER_DEBOUNCE_MS,
    );
    debounce_signal_to_effect(
        global_message_query,
        global_message_filter_debounced,
        GLOBAL_MESSAGE_SEARCH_DEBOUNCE_MS,
    );

    view! {
        <>
        <aside class=move || {
            let mut s = String::from("nav-rail");
            if mobile_nav_open.get() {
                s.push_str(" nav-rail-mobile-open");
            }
            s
        }>
            <div class="nav-rail-brand">
                <span class="brand-mark" aria-hidden="true"></span>
                <div class="nav-rail-brand-text">
                    <h1>"CrabMate"</h1>
                    <span class="brand-sub">{move || i18n::brand_sub(locale.get())}</span>
                </div>
            </div>
            <button
                type="button"
                class="btn btn-primary btn-new-chat-ds"
                on:click={
                    let new_session = new_session.clone();
                    move |_| {
                        new_session();
                        mobile_nav_open.set(false);
                    }
                }
            >
                {move || i18n::nav_new_chat(locale.get())}
            </button>
            <button
                type="button"
                class="btn btn-nav-ghost-ds"
                on:click=move |_| {
                    session_modal.set(true);
                    mobile_nav_open.set(false);
                }
            >
                {move || i18n::nav_manage_sessions(locale.get())}
            </button>
            <div class="nav-rail-search">
                <label class="nav-rail-search-label" for="nav-session-filter">{move || i18n::nav_filter_sessions(locale.get())}</label>
                <input
                    id="nav-session-filter"
                    type="search"
                    class="nav-session-search-input"
                    prop:placeholder=move || i18n::nav_ph_filter(locale.get())
                    prop:value=move || sidebar_session_query.get()
                    on:input=move |ev| {
                        sidebar_session_query.set(event_target_value(&ev));
                    }
                />
                <label class="nav-rail-search-label" for="nav-msg-search">{move || i18n::nav_search_messages(locale.get())}</label>
                <input
                    id="nav-msg-search"
                    type="search"
                    class="nav-global-search-input"
                    prop:placeholder=move || i18n::nav_ph_global_search(locale.get())
                    prop:value=move || global_message_query.get()
                    on:input=move |ev| {
                        global_message_query.set(event_target_value(&ev));
                    }
                />
            </div>
            <div class="nav-rail-scroll">
                <div class="nav-rail-scroll-label">{move || i18n::nav_recent(locale.get())}</div>
                {move || {
                    let needle = normalize_search_query(&sidebar_filter_debounced.get());
                    let msg_needle = normalize_search_query(&global_message_filter_debounced.get());
                    let mut v: Vec<ChatSession> = sessions
                        .get()
                        .into_iter()
                        .filter(|s| session_title_matches(s, &needle))
                        .collect();
                    v.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
                    let hits = if msg_needle.is_empty() {
                        Vec::new()
                    } else {
                        sessions.with(|list| {
                            collect_message_search_hits(
                                list,
                                &msg_needle,
                                MESSAGE_SEARCH_MAX_HITS,
                                locale.get(),
                            )
                        })
                    };
                    let hit_views = if !msg_needle.is_empty() {
                        if hits.is_empty() {
                            view! {
                                <div class="nav-search-hits-empty" role="status">
                                    {move || i18n::nav_no_message_hits(locale.get())}
                                </div>
                            }
                            .into_any()
                        } else {
                            hits
                                .into_iter()
                                .map(|h| {
                                    let sid = h.session_id.clone();
                                    let mid = h.message_id.clone();
                                    let title = h.session_title.clone();
                                    let snip = h.snippet.clone();
                                    let buf_hit = Arc::clone(&composer_buf_nav);
                                    view! {
                                        <button
                                            type="button"
                                            class="nav-search-hit"
                                            on:click=move |_| {
                                                let prev = active_id.get_untracked();
                                                if !prev.is_empty() {
                                                    let t = buf_hit.lock().unwrap().clone();
                                                    flush_composer_draft_to_session(
                                                        sessions,
                                                        &prev,
                                                        &t,
                                                    );
                                                }
                                                session_context_menu.set(None);
                                                active_id.set(sid.clone());
                                                draft.set(
                                                    sessions.with(|list| {
                                                        list.iter()
                                                            .find(|s| s.id == sid)
                                                            .map(|s| s.draft.clone())
                                                            .unwrap_or_default()
                                                    }),
                                                );
                                                conversation_id.set(None);
                                                conversation_revision.set(None);
                                                focus_message_id_after_nav.set(Some(mid.clone()));
                                                mobile_nav_open.set(false);
                                            }
                                        >
                                            <span class="nav-search-hit-title">
                                                {move || {
                                                    i18n::session_title_for_display(&title, locale.get())
                                                }}
                                            </span>
                                            <span class="nav-search-hit-snippet">{snip}</span>
                                        </button>
                                    }
                                })
                                .collect_view()
                                .into_any()
                        }
                    } else {
                        ().into_any()
                    };
                    view! {
                        <div class="nav-search-hits" role="region" prop:aria-label=move || i18n::nav_search_hits_region(locale.get())>
                            {hit_views}
                        </div>
                        {v.into_iter()
                        .map(|s| {
                            let session_id_class = s.id.clone();
                            let session_id_click = s.id.clone();
                            let session_id_ctx = s.id.clone();
                            let title = s.title.clone();
                            let n = s.messages.len();
                            let buf_sess = Arc::clone(&composer_buf_nav);
                            view! {
                                <button
                                    type="button"
                                    class=move || {
                                        if active_id.get() == session_id_class {
                                            "nav-session-item is-active"
                                        } else {
                                            "nav-session-item"
                                        }
                                    }
                                    on:contextmenu=move |ev: web_sys::MouseEvent| {
                                        ev.prevent_default();
                                        ev.stop_propagation();
                                        let (x, y) = clamp_session_ctx_menu_pos(
                                            ev.client_x(),
                                            ev.client_y(),
                                        );
                                        session_context_menu.set(Some(SessionContextAnchor {
                                            session_id: session_id_ctx.clone(),
                                            x,
                                            y,
                                        }));
                                    }
                                    on:click={
                                        let id = session_id_click;
                                        move |_| {
                                            let prev = active_id.get_untracked();
                                            if !prev.is_empty() {
                                                let t = buf_sess.lock().unwrap().clone();
                                                flush_composer_draft_to_session(
                                                    sessions,
                                                    &prev,
                                                    &t,
                                                );
                                            }
                                            session_context_menu.set(None);
                                            active_id.set(id.clone());
                                            draft.set(
                                                sessions.with(|list| {
                                                    list.iter()
                                                        .find(|s| s.id == id)
                                                        .map(|s| s.draft.clone())
                                                        .unwrap_or_default()
                                                }),
                                            );
                                            conversation_id.set(None);
                                            conversation_revision.set(None);
                                            mobile_nav_open.set(false);
                                        }
                                    }
                                >
                                    <span class="nav-session-title">
                                        {move || {
                                            i18n::session_title_for_display(&title, locale.get())
                                        }}
                                    </span>
                                    <span class="nav-session-meta">{move || i18n::session_row_msg_count(locale.get(), n)}</span>
                                </button>
                            }
                        })
                        .collect_view()}
                    }
                    .into_any()
                }}
            </div>
        </aside>

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
                        let anchor = session_context_menu.get();
                        let Some(a) = anchor else {
                            return;
                        };
                        let id = a.session_id;
                        session_context_menu.set(None);
                        export_session_json_for_id(sessions, &id, locale.get_untracked());
                    }
                >
                    {move || i18n::ctx_export_json(locale.get())}
                </button>
                <button
                    type="button"
                    class="session-ctx-item"
                    role="menuitem"
                    on:click=move |_| {
                        let anchor = session_context_menu.get();
                        let Some(a) = anchor else {
                            return;
                        };
                        let id = a.session_id;
                        session_context_menu.set(None);
                        export_session_markdown_for_id(sessions, &id, locale.get_untracked());
                    }
                >
                    {move || i18n::ctx_export_md(locale.get())}
                </button>
                <button
                    type="button"
                    class="session-ctx-item session-ctx-item-danger"
                    role="menuitem"
                    on:click=move |_| {
                        let anchor = session_context_menu.get();
                        let Some(a) = anchor else {
                            return;
                        };
                        let id = a.session_id;
                        session_context_menu.set(None);
                        delete_session_after_confirm(
                            sessions,
                            active_id,
                            draft,
                            conversation_id,
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

        <Show when=move || mobile_nav_open.get()>
            <div
                class="nav-rail-backdrop"
                aria-hidden="true"
                on:click=move |_| mobile_nav_open.set(false)
            ></div>
        </Show>
        </>
    }
}
