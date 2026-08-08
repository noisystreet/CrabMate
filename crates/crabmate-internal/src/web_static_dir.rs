//! Web 静态资源目录：可选 SPA `dist`（官方 UI 在 Client 仓 `crabmate-client/frontend`）。
//!
//! 解析顺序：`CM_WEB_STATIC_DIR` → 自 crate/可执行文件/cwd 向上查找已构建的
//! `frontend/dist` 或同级 `crabmate-client/frontend/dist` → 安装布局
//! [`INSTALLED_FRONTEND_DIST`] → 约定路径（可能尚不存在，供错误提示）。

use std::path::{Path, PathBuf};

/// 桌面 `.deb` / 系统安装时 `serve` 提供 Web UI 的静态资源根（含 `vendor/ide-codemirror.js`）。
pub const INSTALLED_FRONTEND_DIST: &str = "/usr/share/crabmate/frontend/dist";

/// 解析 `serve` 与 `config --dry-run` 使用的静态资源根目录。
pub fn resolve_web_static_dir() -> PathBuf {
    if let Some(dist) = env_frontend_dist() {
        return dist;
    }
    if let Some(dist) = find_built_frontend_dist_from(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        return dist;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
        && let Some(dist) = find_built_frontend_dist_from(parent)
    {
        return dist;
    }
    if let Ok(cwd) = std::env::current_dir()
        && let Some(dist) = find_built_frontend_dist_from(&cwd)
    {
        return dist;
    }
    if let Some(dist) = installed_frontend_dist() {
        return dist;
    }
    conventional_frontend_dist_hint()
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
    // 开发机已装 deb 时，环境可能仍导出安装路径；cwd 在源码树且本地/Client dist 已构建则优先本地。
    if path.as_path() == Path::new(INSTALLED_FRONTEND_DIST)
        && let Some(dev) = frontend_dist_from_cwd_if_built()
    {
        return Some(dev);
    }
    Some(path)
}

/// 自进程 `current_dir` 向上查找已构建的 UI dist（须含 `index.html`）。
fn frontend_dist_from_cwd_if_built() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    find_built_frontend_dist_from(&cwd)
}

fn installed_frontend_dist() -> Option<PathBuf> {
    let path = PathBuf::from(INSTALLED_FRONTEND_DIST);
    is_frontend_dist(&path).then_some(path)
}

fn is_frontend_dist(path: &Path) -> bool {
    path.join("index.html").is_file()
}

fn candidate_dist_dirs(dir: &Path) -> [PathBuf; 2] {
    [
        dir.join("frontend/dist"),
        dir.join("crabmate-client/frontend/dist"),
    ]
}

fn find_built_frontend_dist_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        for cand in candidate_dist_dirs(&dir) {
            if is_frontend_dist(&cand) {
                return Some(cand);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// 无已构建 dist 时的约定提示路径（相对本 workspace 根的 `frontend/dist`）。
fn conventional_frontend_dist_hint() -> PathBuf {
    find_workspace_root(Path::new(env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .join("frontend/dist")
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file()
            && let Ok(text) = std::fs::read_to_string(&manifest)
            && text.contains("[workspace]")
        {
            return Some(dir);
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
    fn resolve_returns_path_ending_with_frontend_dist() {
        let dist = resolve_web_static_dir();
        assert!(dist.ends_with("frontend/dist"), "got {}", dist.display());
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
    fn workspace_root_detection_from_internal_crate() {
        let root = find_workspace_root(Path::new(env!("CARGO_MANIFEST_DIR")))
            .expect("workspace root from crabmate-internal");
        assert!(root.join("Cargo.toml").is_file());
        assert!(
            std::fs::read_to_string(root.join("Cargo.toml"))
                .expect("read")
                .contains("[workspace]")
        );
    }

    #[test]
    fn installed_env_defers_to_built_cwd_dist_when_available() {
        let Some(dev) = frontend_dist_from_cwd_if_built() else {
            return;
        };
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
            dev,
            "expected built cwd/client dist, got {}",
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
