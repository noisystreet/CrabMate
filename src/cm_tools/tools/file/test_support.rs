//! `file` 子模块测试共用的夹具（仅 `#[cfg(test)]`）。
//!
//! 实现已下沉到 [`crate::cm_tools::tools::testing`]；此处仅保留 `pub(super)` 别名，
//! 避免 `file` 下各测试模块同时改动 import。

pub(super) use crate::cm_tools::tools::testing::{make_test_dir, test_ctx, write};
