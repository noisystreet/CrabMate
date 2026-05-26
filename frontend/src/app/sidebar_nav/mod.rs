//! 左侧导航：品牌、新对话、会话筛选与列表、全文搜索命中、会话右键菜单。
//!
//! 「筛选会话」「搜索消息」输入区默认收起，在会话列表空白处右键打开；展开后输入仍即时写入
//! `sidebar_session_query` / `global_message_query`；列表与 `collect_message_search_hits` 使用防抖后的副本，
//! 避免每次 `input` 全量遍历。收起时列表不按搜索条件过滤。

mod context_menus;
mod debounce;
mod editor_layout_toggle;
mod search_panel;
mod session_rail;

use leptos::prelude::*;

use crate::session_ops::clamp_session_ctx_menu_pos;

use super::app_shell_ctx::SidebarNavSignals;

use super::shell_runtime_context::expect_chat_shell_ctx;

use context_menus::{RailContextMenuLayer, SessionContextMenuLayer};
use debounce::{
    GLOBAL_MESSAGE_SEARCH_DEBOUNCE_MS, SIDEBAR_SESSION_FILTER_DEBOUNCE_MS,
    debounce_signal_to_effect, rail_context_menu_target_is_session_row_or_hit,
};
use editor_layout_toggle::NavRailEditorLayoutToggle;
use search_panel::nav_rail_search_panel;
use session_rail::{NavRailSessionScrollSignals, nav_rail_session_scroll_inner};

pub fn sidebar_nav_view(signals: SidebarNavSignals) -> impl IntoView {
    let SidebarNavSignals {
        locale,
        mobile_nav_open,
        session_modal,
        new_session,
        sidebar_session_query,
        global_message_query,
        sidebar_search_panel_open,
        sidebar_rail_ctx_menu,
        chat_find_panel_open,
        session_context_menu,
        sidebar_rail_collapsed,
        editor_layout_mode,
    } = signals;
    let shell_chat = expect_chat_shell_ctx();
    let chat = shell_chat.chat;
    let draft = shell_chat.composer.draft;
    let focus_message_id_after_nav = shell_chat.composer.focus_message_id_after_nav;
    let apply_assistant_display_filters = shell_chat.apply_assistant_display_filters;
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
                <div class="nav-rail-brand-main">
                    <span class="brand-mark" aria-hidden="true"></span>
                    <div class="nav-rail-brand-text">
                        <h1>"CrabMate"</h1>
                    </div>
                </div>
                <button
                    type="button"
                    class="btn btn-icon btn-nav-rail-collapse"
                    prop:aria-label=move || crate::i18n::nav_sidebar_collapse_aria(locale.get())
                    prop:aria-expanded=move || (!sidebar_rail_collapsed.get()).to_string()
                    on:click=move |_| sidebar_rail_collapsed.set(true)
                >
                    "‹"
                </button>
            </div>
            <button
                type="button"
                class="btn btn-primary btn-new-chat-ds"
                data-testid="nav-new-chat"
                on:click={
                    let new_session = new_session.clone();
                    move |_| {
                        new_session();
                        mobile_nav_open.set(false);
                    }
                }
            >
                {move || crate::i18n::nav_new_chat(locale.get())}
            </button>
            <NavRailEditorLayoutToggle locale=locale editor_layout_mode=editor_layout_mode />
            {nav_rail_search_panel(
                locale,
                sidebar_search_panel_open,
                sidebar_session_query,
                global_message_query,
            )}
            <div
                class="nav-rail-scroll"
                prop:title=move || crate::i18n::nav_rail_scroll_search_hint(locale.get())
                on:contextmenu=move |ev: web_sys::MouseEvent| {
                    if rail_context_menu_target_is_session_row_or_hit(&ev) {
                        return;
                    }
                    ev.prevent_default();
                    ev.stop_propagation();
                    session_context_menu.set(None);
                    let (x, y) = clamp_session_ctx_menu_pos(ev.client_x(), ev.client_y());
                    sidebar_rail_ctx_menu.set(Some((x, y)));
                }
            >
                <div class="nav-rail-scroll-label">{move || crate::i18n::nav_recent(locale.get())}</div>
                {nav_rail_session_scroll_inner(NavRailSessionScrollSignals {
                    locale,
                    sidebar_search_panel_open,
                    sidebar_filter_debounced,
                    global_message_filter_debounced,
                    chat,
                    draft,
                    mobile_nav_open,
                    session_context_menu,
                    sidebar_rail_ctx_menu,
                    focus_message_id_after_nav,
                    apply_assistant_display_filters,
                })}
            </div>
        </aside>

        <SessionContextMenuLayer
            session_context_menu=session_context_menu
            session_modal=session_modal
            mobile_nav_open=mobile_nav_open
        />

        <RailContextMenuLayer
            locale=locale
            sidebar_rail_ctx_menu=sidebar_rail_ctx_menu
            session_modal=session_modal
            mobile_nav_open=mobile_nav_open
            sidebar_search_panel_open=sidebar_search_panel_open
            chat_find_panel_open=chat_find_panel_open
        />

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
