//! IDE 菜单栏信号 bundle（避免组件形参过多）。

use leptos::prelude::*;

use crate::app::app_signals::{IdeChromeSignals, IdeEditorSignals};
use crate::i18n::Locale;
use crate::ide_save::IdeSaveContext;
use crate::ide_tabs::IdeTabsHandle;

#[derive(Clone, Copy)]
pub struct IdeMenuBarSignals {
    pub locale: RwSignal<Locale>,
    pub chrome: IdeChromeSignals,
    pub editor: IdeEditorSignals,
    pub editor_layout_mode: RwSignal<bool>,
    pub ide_settings_page: RwSignal<bool>,
    pub ide_menubar_dropdown_open: RwSignal<bool>,
    pub ide_path: RwSignal<Option<String>>,
    pub ide_text: RwSignal<String>,
    pub ide_baseline: RwSignal<String>,
    pub ide_load_busy: RwSignal<bool>,
    pub ide_save_busy: RwSignal<bool>,
    pub textarea_ref: NodeRef<leptos::html::Textarea>,
    pub tabs: IdeTabsHandle,
    pub save_ctx: IdeSaveContext,
}
