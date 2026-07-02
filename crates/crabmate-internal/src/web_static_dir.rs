//! Web 静态资源目录：仓库内 **`frontend/dist`**（Leptos + Trunk），或安装布局
//! **`/usr/share/crabmate/frontend/dist`**（桌面 `.deb` 等）。
//!
//! 自 `crabmate-internal` 拆出后，`CARGO_MANIFEST_DIR` 指向子 crate；自该路径、可执行文件目录与
//! 当前工作目录向上查找含 **`frontend/Trunk.toml`** 或 **`frontend/dist/index.html`** 的工作区根。

use std::path::{Path, PathBuf};

/// 桌面 `.deb` / 系统安装时 `serve` 提供 Web UI 的静态资源根（含 `vendor/ide-codemirror.js`）。
pub const INSTALLED_FRONTEND_DIST: &str = "/usr/share/crabmate/frontend/dist";

/// 解析 `serve` 与 `config --dry-run` 使用的静态资源根目录。
pub fn resolve_web_static_dir() -> PathBuf {
    if let Some(dist) = env_frontend_dist() {
        return dist;
    }
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
    if let Some(dist) = installed_frontend_dist() {
        return dist;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist")
}

fn env_frontend_dist() -> Option<PathBuf> {
    let raw = std::env::var("CM_WEB_STATIC_DIR").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if !is_frontend_dist(&path) {
        return None;
    }
    // 开发机已装 deb 时，shell/旧 sidecar 可能仍导出安装路径；cwd 在源码树且本地 dist 已构建则优先本地。
    if path.as_path() == Path::new(INSTALLED_FRONTEND_DIST)
        && let Some(dev) = frontend_dist_from_cwd_if_built()
    {
        return Some(dev);
    }
    Some(path)
}

/// 自进程 `current_dir` 向上查找已构建的 `frontend/dist`（须含 `index.html`）。
fn frontend_dist_from_cwd_if_built() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let dist = find_frontend_dist_from(&cwd)?;
    is_frontend_dist(&dist).then_some(dist)
}

fn installed_frontend_dist() -> Option<PathBuf> {
    let path = PathBuf::from(INSTALLED_FRONTEND_DIST);
    is_frontend_dist(&path).then_some(path)
}

fn is_frontend_dist(path: &Path) -> bool {
    path.join("index.html").is_file()
}

fn find_frontend_dist_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let dist = dir.join("frontend/dist");
        if is_frontend_dist(&dist) || dir.join("frontend/Trunk.toml").is_file() {
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

    #[test]
    fn installed_path_constant_matches_share_layout() {
        assert!(INSTALLED_FRONTEND_DIST.ends_with("frontend/dist"));
        assert!(INSTALLED_FRONTEND_DIST.starts_with("/usr/share/"));
    }

    #[test]
    fn is_frontend_dist_requires_index_html() {
        let tmp =
            std::env::temp_dir().join(format!("crabmate_web_static_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        assert!(!is_frontend_dist(&tmp));
        std::fs::write(tmp.join("index.html"), b"<!DOCTYPE html>").expect("write index");
        assert!(is_frontend_dist(&tmp));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn installed_env_defers_to_built_cwd_dist_when_in_repo() {
        let dist = resolve_web_static_dir();
        let workspace = dist
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        if !workspace.join("frontend/Trunk.toml").is_file() || !is_frontend_dist(&dist) {
            return;
        }
        let installed = PathBuf::from(INSTALLED_FRONTEND_DIST);
        if !is_frontend_dist(&installed) {
            return;
        }
        let prev = std::env::var("CM_WEB_STATIC_DIR").ok();
        unsafe {
            std::env::set_var("CM_WEB_STATIC_DIR", INSTALLED_FRONTEND_DIST);
        }
        let resolved = super::env_frontend_dist().expect("env dist");
        assert_eq!(
            resolved,
            dist,
            "expected repo frontend/dist, got {}",
            resolved.display()
        );
        unsafe {
            match prev {
                Some(v) => std::env::set_var("CM_WEB_STATIC_DIR", v),
                None => std::env::remove_var("CM_WEB_STATIC_DIR"),
            }
        }
    }
}
