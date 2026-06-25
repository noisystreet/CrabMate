//! IDE 菜单栏信号 bundle（避免组件形参过多）。

use std::sync::Arc;

use leptos::prelude::*;

use crate::app::app_signals::IdeEditorSignals;
use crate::i18n::Locale;
use crate::ide_tabs::IdeTabsHandle;
use crate::workspace_context_menu::WorkspaceTreeRefreshHint;

#[derive(Clone, Copy)]
pub struct IdeMenuBarSignals {
    pub locale: RwSignal<Locale>,
    pub editor: IdeEditorSignals,
    pub editor_layout_mode: RwSignal<bool>,
    pub ide_settings_page: RwSignal<bool>,
    pub ide_menubar_dropdown_open: RwSignal<bool>,
    pub ide_path: RwSignal<Option<String>>,
    pub ide_text: RwSignal<String>,
    pub ide_baseline: RwSignal<String>,
    pub ide_load_busy: RwSignal<bool>,
    pub ide_save_busy: RwSignal<bool>,
    pub ide_err: RwSignal<Option<String>>,
    pub textarea_ref: NodeRef<leptos::html::Textarea>,
    pub tabs: IdeTabsHandle,
    pub refresh_after_mutation: StoredValue<Arc<dyn Fn(WorkspaceTreeRefreshHint) + Send + Sync>>,
}
