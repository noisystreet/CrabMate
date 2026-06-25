//! 对话 / 编辑器主区布局切换（侧栏与窄屏顶栏共用单键切换）。

use leptos::prelude::*;

use crate::i18n::{self, Locale};

#[component]
pub fn LayoutModeSegment(
    locale: RwSignal<Locale>,
    editor_layout_mode: RwSignal<bool>,
    #[prop(default = "")] extra_class: &'static str,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class=format!("layout-mode-toggle-btn {extra_class}")
            data-testid="layout-mode-toggle"
            prop:aria-controls="layout-mode-panel-main"
            prop:aria-label=move || {
                i18n::ide_layout_toggle_aria(locale.get(), editor_layout_mode.get())
            }
            prop:title=move || {
                i18n::ide_layout_toggle_aria(locale.get(), editor_layout_mode.get())
            }
            on:click=move |_| editor_layout_mode.update(|on| *on = !*on)
        >
            {move || i18n::ide_layout_toggle_label(locale.get(), editor_layout_mode.get())}
        </button>
    }
}
