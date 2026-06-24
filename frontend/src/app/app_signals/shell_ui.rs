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
    /// IDE 菜单栏任一下拉打开时为 `true`（供全局 Escape 关闭）。
    pub ide_menubar_dropdown_open: RwSignal<bool>,
    pub status_bar_visible: RwSignal<bool>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub side_width: RwSignal<f64>,
    pub web_ui_config_loaded: RwSignal<bool>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    /// `true` 时主区为 IDE 式（工作区树 + 编辑器），隐藏对话列与右列。
    pub editor_layout_mode: RwSignal<bool>,
    /// 递增时 IDE 布局保存当前活动标签（`Ctrl/Cmd+S`）。
    pub ide_save_active_nonce: RwSignal<u64>,
    /// 递增时 IDE 布局保存全部脏标签（`Ctrl/Cmd+Shift+S`）。
    pub ide_save_all_nonce: RwSignal<u64>,
    /// 递增时 IDE 布局从磁盘重载已打开文件（`workspace_changed` 等）。
    pub ide_sync_disk_nonce: RwSignal<u64>,
    /// 会话模式壳层 UI 字体 slug（`default` 表示随主题 `--font-sans`）。
    pub session_ui_font: RwSignal<String>,
    /// 聊天列消息与输入框正文字体 slug（`code`/`pre` 仍用 `--font-mono`）。
    pub session_chat_font: RwSignal<String>,
}

impl ShellUISignals {
    pub fn new() -> Self {
        let s = shell_prefs_storage::read_shell_ui_initial_snapshot();
        Self {
            theme: RwSignal::new(s.theme),
            bg_decor: RwSignal::new(s.bg_decor),
            locale: RwSignal::new(s.locale),
            view_menu_open: RwSignal::new(false),
            ide_menubar_dropdown_open: RwSignal::new(false),
            status_bar_visible: RwSignal::new(s.status_bar_visible),
            side_panel_view: RwSignal::new(s.side_panel_view),
            side_width: RwSignal::new(s.side_width),
            web_ui_config_loaded: RwSignal::new(false),
            markdown_render: RwSignal::new(true),
            apply_assistant_display_filters: RwSignal::new(true),
            editor_layout_mode: RwSignal::new(s.editor_layout_mode),
            ide_save_active_nonce: RwSignal::new(0),
            ide_save_all_nonce: RwSignal::new(0),
            ide_sync_disk_nonce: RwSignal::new(0),
            session_ui_font: RwSignal::new(s.session_ui_font),
            session_chat_font: RwSignal::new(s.session_chat_font),
        }
    }
}

impl Default for ShellUISignals {
    fn default() -> Self {
        Self::new()
    }
}
