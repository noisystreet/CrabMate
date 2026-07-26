//! 侧栏品牌行右侧：「新建对话」与收起会话栏。

use std::rc::Rc;

use leptos::prelude::*;

use crate::i18n::{self, Locale};

#[component]
pub(super) fn NavRailBrandActions(
    locale: RwSignal<Locale>,
    new_session: Rc<dyn Fn()>,
    mobile_nav_open: RwSignal<bool>,
    sidebar_rail_collapsed: RwSignal<bool>,
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
                prop:aria-expanded=move || (!sidebar_rail_collapsed.get()).to_string()
                on:click=move |_| sidebar_rail_collapsed.set(true)
            >
                "‹"
            </button>
        </div>
    }
}
