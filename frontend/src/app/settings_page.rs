//! 设置页面：全屏视图与路由/布局子模块（`SettingsPageView` 实现在 `settings_page/view.rs`）。

pub(crate) mod dom_preview;
pub(crate) mod form_snapshot;
mod hash_routing;
mod layout;
pub(crate) mod page_actions;
mod section_copy;
mod view;

pub use view::{SettingsPageFormSignals, SettingsPageView, SettingsPageViewInput};
