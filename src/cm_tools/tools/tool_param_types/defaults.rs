//! serde 默认值辅助：被多个工具族共用（原先靠 `include!` 的同作用域裸名共享，拆分后必须显式成模块）。

/// `#[serde(default = "...")]` 用的布尔默认值 `true`。
pub(super) fn default_true() -> bool {
    true
}
