//! 侧栏品牌行右侧：筛选/搜索、新建对话、收起会话栏。

use std::rc::Rc;

use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::i18n::{self, Locale};

fn focus_nav_session_filter() {
    // Show 挂载后再聚焦；双 rAF 覆盖一批微任务未刷完 DOM 的情况
    request_animation_frame(|| {
        request_animation_frame(|| {
            let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
                return;
            };
            let Some(el) = doc.get_element_by_id("nav-session-filter") else {
                return;
            };
            let Ok(html) = el.dyn_into::<web_sys::HtmlElement>() else {
                return;
            };
            let _ = html.focus();
        });
    });
}

#[component]
pub(super) fn NavRailBrandActions(
    locale: RwSignal<Locale>,
    new_session: Rc<dyn Fn()>,
    mobile_nav_open: RwSignal<bool>,
    sidebar_rail_collapsed: RwSignal<bool>,
    sidebar_search_panel_open: RwSignal<bool>,
) -> impl IntoView {
    let on_new_chat = {
        let new_session = Rc::clone(&new_session);
        move |_| {
            new_session();
            mobile_nav_open.set(false);
        }
    };
    view! {
        <div class="nav-rail-brand-actions">
            <button
                type="button"
                class="btn btn-icon btn-nav-toggle-search"
                class:active=move || sidebar_search_panel_open.get()
                data-testid="nav-toggle-search"
                prop:title=move || {
                    i18n::nav_toggle_search_panel_aria(
                        locale.get(),
                        sidebar_search_panel_open.get(),
                    )
                }
                prop:aria-label=move || {
                    i18n::nav_toggle_search_panel_aria(
                        locale.get(),
                        sidebar_search_panel_open.get(),
                    )
                }
                aria-expanded=move || {
                    if sidebar_search_panel_open.get() {
                        "true"
                    } else {
                        "false"
                    }
                }
                prop:aria-controls="nav-rail-search-panel"
                on:click=move |_| {
                    let opening = !sidebar_search_panel_open.get_untracked();
                    sidebar_search_panel_open.set(opening);
                    if opening {
                        focus_nav_session_filter();
                    }
                }
            >
                <span aria-hidden="true">"⌕"</span>
            </button>
            <button
                type="button"
                class="btn btn-primary btn-icon btn-nav-new-chat"
                data-testid="nav-new-chat"
                prop:title=move || i18n::nav_new_chat(locale.get())
                prop:aria-label=move || i18n::nav_new_chat_aria(locale.get())
                on:click=on_new_chat
            >
                <span aria-hidden="true">"+"</span>
            </button>
            <button
                type="button"
                class="btn btn-icon btn-nav-rail-collapse"
                prop:aria-label=move || crate::i18n::nav_sidebar_collapse_aria(locale.get())
                aria-expanded=move || if !sidebar_rail_collapsed.get() { "true" } else { "false" }
                on:click=move |_| sidebar_rail_collapsed.set(true)
            >
                "‹"
            </button>
        </div>
    }
}
