//! IDE 设置：字体块。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::app::app_signals::IdeEditorSignals;
use crate::i18n::{self, Locale};
use crate::ide_editor_prefs::IDE_EDITOR_FONT_SLUGS;

#[component]
pub(super) fn IdeSettingsFontBlock(
    locale: RwSignal<Locale>,
    editor: IdeEditorSignals,
) -> impl IntoView {
    let font_select_id = "ide-settings-font-family";
    let font_size_id = "ide-settings-font-size";

    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::ide_settings_block_font(locale.get())}</h3>
            <div class="settings-field">
                <label class="settings-field-label" for=font_select_id>
                    {move || i18n::ide_settings_label_font_family(locale.get())}
                </label>
                <select
                    id=font_select_id
                    class="settings-select"
                    prop:value=move || editor.font_slug.get()
                    on:change=move |ev| editor.font_slug.set(event_target_value(&ev))
                >
                    {IDE_EDITOR_FONT_SLUGS.iter().copied().map(|slug| {
                        view! {
                            <option value=slug>
                                {move || i18n::ide_settings_font_label(locale.get(), slug)}
                            </option>
                        }
                    }).collect_view()}
                </select>
            </div>
            <div class="settings-field">
                <label class="settings-field-label" for=font_size_id>
                    {move || i18n::ide_settings_label_font_size(locale.get())}
                </label>
                <input
                    id=font_size_id
                    type="number"
                    class="settings-text-input"
                    min="10"
                    max="28"
                    step="1"
                    prop:value=move || editor.font_size_px.get().round() as i32
                    on:input=move |ev| {
                        if let Ok(n) = event_target_value(&ev).parse::<f64>() {
                            editor.font_size_px.set(n.clamp(10.0, 28.0));
                        }
                    }
                />
            </div>
        </div>
    }
}
