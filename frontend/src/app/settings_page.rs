//! 设置页面：全屏视图与路由/布局子模块（`SettingsPageView` 实现在 `settings_page/view.rs`；壳级 `Effect` 在 **`effects.rs`**，顶栏在 **`header.rs`**）。

pub(crate) mod dom_preview;
mod effects;
pub(crate) mod form_snapshot;
mod hash_routing;
mod header;
mod layout;
pub(crate) mod page_actions;
mod section_copy;
mod view;

pub use view::{SettingsPageFormSignals, SettingsPageView, SettingsPageViewInput};
