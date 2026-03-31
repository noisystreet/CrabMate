//! Web 静态资源目录：仅使用 `frontend-leptos/dist`（Leptos + Trunk）。

use std::path::{Path, PathBuf};

/// 解析 `serve` 与 `config --dry-run` 使用的静态资源根目录。
pub fn resolve_web_static_dir() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join("frontend-leptos/dist")
}
