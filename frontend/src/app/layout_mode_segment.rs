//! 对话 / 编辑器主区布局切换（统一壳顶栏最左侧单键切换）。

use leptos::prelude::*;

use crate::app::ide_layout_switch::{IdeLayoutToggleSignals, toggle_editor_layout};
use crate::i18n::{self, Locale};

#[component]
pub fn LayoutModeSegment(
    locale: RwSignal<Locale>,
    layout_toggle: IdeLayoutToggleSignals,
    #[prop(default = "")] extra_class: &'static str,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class=format!("layout-mode-toggle-btn {extra_class}")
            data-testid="layout-mode-toggle"
            prop:aria-controls="layout-mode-panel-main"
            prop:aria-label=move || {
                i18n::ide_layout_toggle_aria(locale.get(), layout_toggle.editor_layout_mode.get())
            }
            prop:title=move || {
                i18n::ide_layout_toggle_aria(locale.get(), layout_toggle.editor_layout_mode.get())
            }
            on:click=move |_| toggle_editor_layout(layout_toggle)
        >
            {move || {
                i18n::ide_layout_toggle_label(locale.get(), layout_toggle.editor_layout_mode.get())
            }}
        </button>
    }
}
