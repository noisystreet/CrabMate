//! Web 静态资源目录：`frontend-leptos/dist` 优先（存在则用于 Leptos WASM 构建产物），否则 `frontend/dist`（Vite/React）。

use std::path::{Path, PathBuf};

/// 解析 `serve` 与 `config --dry-run` 使用的静态资源根目录。
pub fn resolve_web_static_dir() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let leptos = root.join("frontend-leptos/dist");
    if leptos.is_dir() {
        leptos
    } else {
        root.join("frontend/dist")
    }
}
