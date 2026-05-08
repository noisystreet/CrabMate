//! `App` 级副作用：首启加载会话、`localStorage` 与 DOM 偏好同步、`Escape` 分层关闭等。
//!
//! 从 `app/mod.rs` 抽出，使根组件以「声明信号 + 调用 `wire_*`」为主。
//!
//! ## 子模块与「隐式订阅」边界
//!
//! - [`session_storage`]：`sessions` / `active_id` 持久化与首启加载——与聊天流式正文高频更新同信号域，勿在此处叠加无关逻辑。
//! - [`escape`]：`keydown` 回调内统一 **`get_untracked()`** 读面板开关，避免单个巨型 `Effect` 订阅全部 UI 状态。
//! - [`settings_llm_open`]：仅在设置弹窗/页面打开时填充草稿；**`status_data` 用 `get_untracked`**，避免订阅 `/status` 刷新导致重复填充。
//!
//! **注册顺序**仍见 [`super::app_shell_bootstrap::bootstrap_app_shell`]（及 [`super::app_shell_init::init_app_shell`]）。

mod approval_follow;
mod escape;
mod persist_prefs;
mod session_storage;
mod settings_llm_open;
mod sync_dom;

pub use approval_follow::wire_approval_expanded_follows_pending;
pub use escape::{ShellEscapeSignals, wire_escape_key_layered_dismiss};
pub use persist_prefs::{
    wire_persist_agent_role, wire_persist_side_panel_view_flags, wire_persist_side_width,
    wire_persist_sidebar_rail_collapsed, wire_persist_status_bar_visible,
};
pub use session_storage::{
    wire_initial_sessions_from_storage, wire_persist_chat_sessions,
    wire_web_ui_config_once_after_init,
};
pub use settings_llm_open::{
    WireSettingsModalLlmDraftsSignals, wire_settings_modal_llm_drafts_on_open,
};
pub use sync_dom::{
    wire_sync_bg_decor_to_storage_and_dom, wire_sync_locale_html_lang,
    wire_sync_theme_to_storage_and_dom,
};
