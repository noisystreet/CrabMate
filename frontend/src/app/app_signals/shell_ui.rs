//! 壳 UI：主题、语言、侧栏视图、Web UI 展示偏好等。

use leptos::prelude::*;

use crate::app::shell_prefs_storage;
use crate::app_prefs::SidePanelView;
use crate::i18n::Locale;

#[derive(Clone, Copy)]
pub struct ShellUISignals {
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub view_menu_open: RwSignal<bool>,
    pub status_bar_visible: RwSignal<bool>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub side_width: RwSignal<f64>,
    pub web_ui_config_loaded: RwSignal<bool>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
}

impl ShellUISignals {
    pub fn new() -> Self {
        let s = shell_prefs_storage::read_shell_ui_initial_snapshot();
        Self {
            theme: RwSignal::new(s.theme),
            bg_decor: RwSignal::new(s.bg_decor),
            locale: RwSignal::new(s.locale),
            view_menu_open: RwSignal::new(false),
            status_bar_visible: RwSignal::new(s.status_bar_visible),
            side_panel_view: RwSignal::new(s.side_panel_view),
            side_width: RwSignal::new(s.side_width),
            web_ui_config_loaded: RwSignal::new(false),
            markdown_render: RwSignal::new(true),
            apply_assistant_display_filters: RwSignal::new(true),
        }
    }
}

impl Default for ShellUISignals {
    fn default() -> Self {
        Self::new()
    }
}
