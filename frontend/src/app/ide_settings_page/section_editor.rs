//! IDE 编辑器设置表单块。

use leptos::prelude::*;

use super::section_display::IdeSettingsDisplayBlock;
use super::section_font::IdeSettingsFontBlock;
use crate::app::app_signals::IdeEditorSignals;
use crate::i18n::Locale;

#[component]
pub(super) fn IdeSettingsEditorBlock(
    locale: RwSignal<Locale>,
    editor: IdeEditorSignals,
) -> impl IntoView {
    view! {
        <IdeSettingsFontBlock locale=locale editor=editor />
        <IdeSettingsDisplayBlock locale=locale editor=editor />
    }
}
