//! IDE 编辑器设置全屏视图。

use leptos::prelude::*;

use super::draft::wire_ide_settings_draft;
use super::section_editor::IdeSettingsEditorBlock;
use super::view_header::IdeSettingsPageHeader;
use crate::app::app_signals::IdeEditorSignals;
use crate::i18n::{self, Locale};

#[derive(Clone, Copy)]
pub struct IdeSettingsPageViewInput {
    pub ide_settings_page: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub editor: IdeEditorSignals,
}

#[derive(Clone, Copy, PartialEq)]
enum IdeSettingsSection {
    Editor,
}

#[component]
pub fn IdeSettingsPageView(input: IdeSettingsPageViewInput) -> impl IntoView {
    let IdeSettingsPageViewInput {
        ide_settings_page,
        locale,
        editor,
    } = input;

    let active_section = RwSignal::new(IdeSettingsSection::Editor);
    let draft = wire_ide_settings_draft(ide_settings_page, editor);

    view! {
        <div class="settings-page" class:settings-page-visible=move || ide_settings_page.get()>
            <IdeSettingsPageHeader
                locale=locale
                dirty=draft.dirty
                on_back=draft.on_back
                discard_rc=draft.discard_rc
                save_rc=draft.save_rc
            />
            <div class="settings-page-body">
                <div class="settings-layout">
                    <nav class="settings-nav" prop:aria-label=move || i18n::ide_settings_nav_aria(locale.get())>
                        <button
                            type="button"
                            class="settings-nav-item"
                            class:active=move || active_section.get() == IdeSettingsSection::Editor
                            on:click=move |_| active_section.set(IdeSettingsSection::Editor)
                        >
                            {move || i18n::ide_settings_section_editor_title(locale.get())}
                        </button>
                    </nav>
                    <section class="settings-content">
                        <header class="settings-content-header">
                            <h2 class="settings-content-title">
                                {move || i18n::ide_settings_section_editor_title(locale.get())}
                            </h2>
                            <p class="settings-content-desc">
                                {move || i18n::ide_settings_section_editor_desc(locale.get())}
                            </p>
                        </header>
                        <IdeSettingsEditorBlock locale=locale editor=draft.draft_editor />
                    </section>
                </div>
            </div>
        </div>
    }
}
