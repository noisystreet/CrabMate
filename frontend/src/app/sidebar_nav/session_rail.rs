use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n;
use crate::session_ops::{
    SessionContextAnchor, clamp_session_ctx_menu_pos, switch_active_session_after_composer_flush,
};
use crate::session_search::{
    MESSAGE_SEARCH_MAX_HITS, MessageSearchHit, collect_message_search_hits, normalize_search_query,
    session_title_matches,
};
use crate::session_sort::sorted_sessions_clone;
use crate::storage::ChatSession;

/// 侧栏会话列表滚动区内共享信号（缩短 [`nav_rail_session_scroll_inner`] 形参列表）。
#[derive(Clone, Copy)]
pub(super) struct NavRailSessionScrollSignals {
    pub(super) locale: RwSignal<crate::i18n::Locale>,
    pub(super) sidebar_search_panel_open: RwSignal<bool>,
    pub(super) sidebar_filter_debounced: RwSignal<String>,
    pub(super) global_message_filter_debounced: RwSignal<String>,
    pub(super) chat: ChatSessionSignals,
    pub(super) draft: RwSignal<String>,
    pub(super) mobile_nav_open: RwSignal<bool>,
    pub(super) session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    pub(super) sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    pub(super) focus_message_id_after_nav: RwSignal<Option<String>>,
    pub(super) apply_assistant_display_filters: RwSignal<bool>,
}

/// 侧栏搜索命中与会话行按钮共享的导航信号（缩短 [`nav_search_hit_button`] / [`nav_session_row_button`] 形参列表）。
#[derive(Clone, Copy)]
pub(super) struct NavRailHitRowNavSignals {
    chat: ChatSessionSignals,
    draft: RwSignal<String>,
    session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    mobile_nav_open: RwSignal<bool>,
    locale: RwSignal<crate::i18n::Locale>,
    focus_message_id_after_nav: RwSignal<Option<String>>,
}

pub(super) fn nav_rail_hit_row_nav_signals_from_scroll(
    s: &NavRailSessionScrollSignals,
) -> NavRailHitRowNavSignals {
    NavRailHitRowNavSignals {
        chat: s.chat,
        draft: s.draft,
        session_context_menu: s.session_context_menu,
        sidebar_rail_ctx_menu: s.sidebar_rail_ctx_menu,
        mobile_nav_open: s.mobile_nav_open,
        locale: s.locale,
        focus_message_id_after_nav: s.focus_message_id_after_nav,
    }
}

pub(super) fn nav_rail_session_scroll_inner(s: NavRailSessionScrollSignals) -> impl IntoView {
    let hit_row_nav = nav_rail_hit_row_nav_signals_from_scroll(&s);
    let NavRailSessionScrollSignals {
        locale,
        sidebar_search_panel_open,
        sidebar_filter_debounced,
        global_message_filter_debounced,
        chat,
        apply_assistant_display_filters,
        ..
    } = s;
    let sessions = chat.sessions;
    move || {
        let search_ui_open = sidebar_search_panel_open.get();
        let needle = if search_ui_open {
            normalize_search_query(&sidebar_filter_debounced.get())
        } else {
            String::new()
        };
        let msg_needle = if search_ui_open {
            normalize_search_query(&global_message_filter_debounced.get())
        } else {
            String::new()
        };
        let v: Vec<ChatSession> = sorted_sessions_clone(&sessions.get())
            .into_iter()
            .filter(|s| session_title_matches(s, &needle))
            .collect();
        let hits = if msg_needle.is_empty() {
            Vec::new()
        } else {
            sessions.with(|list| {
                collect_message_search_hits(
                    list,
                    &msg_needle,
                    MESSAGE_SEARCH_MAX_HITS,
                    locale.get(),
                    apply_assistant_display_filters.get(),
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
                hits.into_iter()
                    .map(|h| nav_search_hit_button(h, hit_row_nav.clone()))
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
                .map(|sess| {
                    nav_session_row_button(sess, hit_row_nav.clone())
                })
                .collect_view()}
        }
        .into_any()
    }
}

fn nav_search_hit_button(h: MessageSearchHit, nav: NavRailHitRowNavSignals) -> impl IntoView {
    let NavRailHitRowNavSignals {
        chat,
        draft,
        session_context_menu,
        sidebar_rail_ctx_menu,
        focus_message_id_after_nav,
        mobile_nav_open,
        locale,
    } = nav;
    let sid = h.session_id.clone();
    let mid = h.message_id.clone();
    let title = h.session_title.clone();
    let snip = h.snippet.clone();
    view! {
        <button
            type="button"
            class="nav-search-hit"
            on:click=move |_| {
                session_context_menu.set(None);
                sidebar_rail_ctx_menu.set(None);
                switch_active_session_after_composer_flush(chat, draft, &sid, true);
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
}

fn nav_session_row_button(s: ChatSession, nav: NavRailHitRowNavSignals) -> impl IntoView {
    let NavRailHitRowNavSignals {
        chat,
        draft,
        session_context_menu,
        sidebar_rail_ctx_menu,
        mobile_nav_open,
        locale,
        ..
    } = nav;
    let active_id = chat.active_id;
    let session_id_class = s.id.clone();
    let session_id_click = s.id.clone();
    let session_id_ctx = s.id.clone();
    let title = s.title.clone();
    let n = s.messages.len();
    let is_pinned = s.pinned;
    let is_starred = s.starred;
    view! {
        <button
            type="button"
            class=move || {
                let mut c = String::from("nav-session-item");
                if active_id.get() == session_id_class {
                    c.push_str(" is-active");
                }
                if is_pinned {
                    c.push_str(" is-pinned");
                }
                if is_starred {
                    c.push_str(" is-starred");
                }
                c
            }
            on:contextmenu=move |ev: web_sys::MouseEvent| {
                ev.prevent_default();
                ev.stop_propagation();
                sidebar_rail_ctx_menu.set(None);
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
                    session_context_menu.set(None);
                    sidebar_rail_ctx_menu.set(None);
                    switch_active_session_after_composer_flush(chat, draft, &id, true);
                    mobile_nav_open.set(false);
                }
            }
        >
            <span class="nav-session-title-row">
                <span class="nav-session-badges">
                    <Show when=move || is_pinned>
                        <span
                            class="nav-session-badge nav-session-badge-pin"
                            aria-hidden="true"
                            prop:title=move || i18n::session_badge_pin_aria(locale.get())
                        >
                            "📌"
                        </span>
                    </Show>
                    <Show when=move || is_starred>
                        <span
                            class="nav-session-badge nav-session-badge-star"
                            aria-hidden="true"
                            prop:title=move || i18n::session_badge_star_aria(locale.get())
                        >
                            "★"
                        </span>
                    </Show>
                </span>
                <span class="nav-session-title">
                    {move || {
                        i18n::session_title_for_display(&title, locale.get())
                    }}
                </span>
            </span>
            <span class="nav-session-meta">{move || i18n::session_row_msg_count(locale.get(), n)}</span>
        </button>
    }
}
