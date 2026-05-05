use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::i18n;

#[allow(clippy::too_many_arguments)]
pub(super) fn nav_rail_search_panel(
    locale: RwSignal<crate::i18n::Locale>,
    sidebar_search_panel_open: RwSignal<bool>,
    sidebar_session_query: RwSignal<String>,
    global_message_query: RwSignal<String>,
) -> impl IntoView {
    view! {
        <Show when=move || sidebar_search_panel_open.get()>
            <div class="nav-rail-search">
                <div class="nav-rail-search-header">
                    <button
                        type="button"
                        class="btn btn-nav-ghost-ds nav-rail-search-hide"
                        prop:aria-label=move || i18n::nav_hide_search_panel_aria(locale.get())
                        on:click=move |_| sidebar_search_panel_open.set(false)
                    >
                        {move || i18n::nav_hide_search_panel(locale.get())}
                    </button>
                </div>
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
        </Show>
    }
}
