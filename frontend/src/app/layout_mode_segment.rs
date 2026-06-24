//! 对话 / 编辑器主区布局分段切换（侧栏与窄屏顶栏共用）。

use leptos::prelude::*;

use crate::i18n::{self, Locale};

#[component]
pub fn LayoutModeSegment(
    locale: RwSignal<Locale>,
    editor_layout_mode: RwSignal<bool>,
    #[prop(default = "")] extra_class: &'static str,
) -> impl IntoView {
    view! {
        <div
            class=format!("layout-mode-segment {extra_class}")
            role="tablist"
            prop:aria-label=move || i18n::layout_mode_segment_aria(locale.get())
        >
            <button
                type="button"
                class="layout-mode-segment-btn"
                role="tab"
                prop:title=move || i18n::ide_toggle_chat_aria(locale.get())
                prop:aria-selected=move || (!editor_layout_mode.get()).to_string()
                prop:id="layout-mode-tab-chat"
                prop:aria-controls="layout-mode-panel-main"
                on:click=move |_| editor_layout_mode.set(false)
            >
                {move || i18n::ide_toggle_chat(locale.get())}
            </button>
            <button
                type="button"
                class="layout-mode-segment-btn"
                role="tab"
                prop:title=move || i18n::ide_toggle_editor_aria(locale.get())
                prop:aria-selected=move || editor_layout_mode.get().to_string()
                prop:id="layout-mode-tab-editor"
                prop:aria-controls="layout-mode-panel-main"
                on:click=move |_| editor_layout_mode.set(true)
            >
                {move || i18n::ide_toggle_editor(locale.get())}
            </button>
        </div>
    }
}
