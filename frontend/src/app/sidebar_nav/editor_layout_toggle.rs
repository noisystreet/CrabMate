//! 侧栏「对话 / 编辑器」主区布局切换（降低 `sidebar_nav_view` 圈复杂度）。

use leptos::prelude::*;

use crate::i18n::{self, Locale};

#[component]
pub(super) fn NavRailEditorLayoutToggle(
    locale: RwSignal<Locale>,
    editor_layout_mode: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="btn btn-secondary btn-new-chat-ds"
            prop:title=move || {
                if editor_layout_mode.get() {
                    i18n::ide_toggle_chat_aria(locale.get())
                } else {
                    i18n::ide_toggle_editor_aria(locale.get())
                }
            }
            prop:aria-pressed=move || editor_layout_mode.get().to_string()
            on:click=move |_| {
                editor_layout_mode.update(|m| *m = !*m);
            }
        >
            {move || {
                if editor_layout_mode.get() {
                    i18n::ide_toggle_chat(locale.get())
                } else {
                    i18n::ide_toggle_editor(locale.get())
                }
            }}
        </button>
    }
}
