//! `App` 级副作用：首启加载会话、`localStorage` 与 DOM 偏好同步、`Escape` 分层关闭等。
//!
//! 从 `app/mod.rs` 抽出，使根组件以「声明信号 + 调用 `wire_*`」为主。
//!
//! ## 子模块与「隐式订阅」边界
//!
//! - **会话 JSON / 首启 / `GET /web-ui`**：已迁至 **`crate::app::chat::session_storage`**，由 **`chat::wire_chat_session_lifecycle`** 按序注册。
//! - [`escape`]：`keydown` 回调内统一 **`get_untracked()`** 读面板开关，避免单个巨型 `Effect` 订阅全部 UI 状态。
//! - [`settings_llm_open`]：仅在设置弹窗/页面打开时填充草稿；**`status_data` 用 `get_untracked`**，避免订阅 `/status` 刷新导致重复填充。
//!
//! **注册顺序**仍见 [`super::app_shell_bootstrap::bootstrap_app_shell`]（及 [`super::app_shell_init::init_app_shell`]）。

mod approval_follow;
mod escape;
mod persist_prefs;
mod session_delete_hotkey;
mod settings_llm_open;
mod sync_dom;

pub use approval_follow::wire_approval_expanded_follows_pending;
pub use escape::{ShellEscapeSignals, wire_escape_key_layered_dismiss};
pub use persist_prefs::{
    wire_close_shell_chrome_when_ide_layout, wire_collapse_sidebar_rail_when_ide_layout,
    wire_persist_agent_role, wire_persist_editor_layout_mode, wire_persist_ide_editor_prefs,
    wire_persist_side_panel_view_flags, wire_persist_side_width,
    wire_persist_sidebar_rail_collapsed, wire_persist_status_bar_visible,
};
pub use session_delete_hotkey::{SessionDeleteHotkeySignals, wire_session_delete_hotkey};
pub use settings_llm_open::{
    WireSettingsModalLlmDraftsSignals, wire_settings_modal_llm_drafts_on_open,
};
pub use sync_dom::{
    wire_sync_bg_decor_to_storage_and_dom, wire_sync_locale_html_lang, wire_sync_tauri_shell_dom,
    wire_sync_theme_to_storage_and_dom,
};
