//! Web 静态资源目录：仓库内 **`frontend/dist`**（Leptos + Trunk）。
//!
//! 自 `crabmate-internal` 拆出后，`CARGO_MANIFEST_DIR` 指向子 crate；自该路径、可执行文件目录与
//! 当前工作目录向上查找含 **`frontend/Trunk.toml`** 或 **`frontend/dist/index.html`** 的工作区根。

use std::path::{Path, PathBuf};

/// 解析 `serve` 与 `config --dry-run` 使用的静态资源根目录。
pub fn resolve_web_static_dir() -> PathBuf {
    if let Some(dist) = find_frontend_dist_from(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        return dist;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
        && let Some(dist) = find_frontend_dist_from(parent)
    {
        return dist;
    }
    if let Ok(cwd) = std::env::current_dir()
        && let Some(dist) = find_frontend_dist_from(&cwd)
    {
        return dist;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist")
}

fn find_frontend_dist_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let dist = dir.join("frontend/dist");
        if dist.join("index.html").is_file() || dir.join("frontend/Trunk.toml").is_file() {
            return Some(dist);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_workspace_frontend_dist_from_internal_crate() {
        let dist = resolve_web_static_dir();
        assert!(dist.ends_with("frontend/dist"), "got {}", dist.display());
        let workspace = dist
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        assert!(
            workspace.join("frontend/Trunk.toml").is_file(),
            "missing Trunk.toml under {}",
            workspace.display()
        );
    }
}
