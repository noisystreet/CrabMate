//! 本机用户数据根目录解析（`CM_CRABMATE_USER_DATA_DIR` / XDG）。

use std::path::{Path, PathBuf};

/// 环境变量覆盖本机用户数据根；否则 `$XDG_DATA_HOME/crabmate` 或 `~/.local/share/crabmate`。
#[must_use]
pub fn user_data_root() -> PathBuf {
    if let Ok(v) = std::env::var("CM_CRABMATE_USER_DATA_DIR") {
        let t = v.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(|h| PathBuf::from(h).join(".local/share"))
        })
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("crabmate")
}

/// 测试用：进程内固定临时 `CM_CRABMATE_USER_DATA_DIR`（与 `store` / secrets 单测共用）。
#[cfg(test)]
#[must_use]
pub(crate) fn ensure_test_user_data_root() -> PathBuf {
    use std::sync::{Mutex, OnceLock};
    static SLOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    let slot = SLOT.get_or_init(|| Mutex::new(None));
    let mut g = slot.lock().unwrap();
    if g.is_none() {
        let dir =
            std::env::temp_dir().join(format!("crabmate-user-data-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // SAFETY: 测试进程内独占临时目录，无并发读写该环境变量。
        unsafe {
            std::env::set_var("CM_CRABMATE_USER_DATA_DIR", dir.display().to_string());
        }
        *g = Some(dir);
    }
    g.clone().unwrap()
}

/// 与前端 `normalize_workspace_partition_path` 一致。
#[must_use]
pub fn normalize_workspace_partition_path(path: &str) -> String {
    path.trim().trim_end_matches('/').to_string()
}

/// `prefs.recent_workspace_roots` 上限（与前端菜单一致）。
pub const RECENT_WORKSPACE_ROOTS_MAX: usize = 10;

/// 将规范路径推到最近列表最前（去重、截断）；空路径忽略。
pub fn push_recent_workspace_root(list: &mut Vec<String>, path: &str) {
    let norm = normalize_workspace_partition_path(path);
    if norm.is_empty() {
        return;
    }
    list.retain(|p| p != &norm);
    list.insert(0, norm);
    if list.len() > RECENT_WORKSPACE_ROOTS_MAX {
        list.truncate(RECENT_WORKSPACE_ROOTS_MAX);
    }
}

/// 非空工作区根 → SHA256 hex；空表示 legacy 全局桶。
#[must_use]
pub fn workspace_partition_hash(workspace_root: &str) -> Option<String> {
    let n = normalize_workspace_partition_path(workspace_root);
    if n.is_empty() {
        return None;
    }
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(n.as_bytes());
    Some(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[must_use]
pub fn global_sessions_path(root: &Path) -> PathBuf {
    root.join("global").join("web_sessions.json")
}

#[must_use]
pub fn workspace_dir(root: &Path, hash: &str) -> PathBuf {
    root.join("workspaces").join(hash)
}

#[must_use]
pub fn workspace_sessions_path(root: &Path, hash: &str) -> PathBuf {
    workspace_dir(root, hash).join("web_sessions.json")
}

#[must_use]
pub fn workspace_manifest_path(root: &Path, hash: &str) -> PathBuf {
    workspace_dir(root, hash).join("manifest.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_empty_workspace_is_none() {
        assert!(workspace_partition_hash("").is_none());
        assert!(workspace_partition_hash("  ").is_none());
    }

    #[test]
    fn hash_trims_trailing_slash() {
        let a = workspace_partition_hash("/tmp/ws");
        let b = workspace_partition_hash("/tmp/ws/");
        assert_eq!(a, b);
    }

    #[test]
    fn push_recent_workspace_root_dedupes_and_caps() {
        let mut list = Vec::new();
        push_recent_workspace_root(&mut list, "/a/");
        push_recent_workspace_root(&mut list, "/b");
        push_recent_workspace_root(&mut list, "/a");
        assert_eq!(list, vec!["/a".to_string(), "/b".to_string()]);
        for i in 0..20 {
            push_recent_workspace_root(&mut list, &format!("/p{i}"));
        }
        assert_eq!(list.len(), RECENT_WORKSPACE_ROOTS_MAX);
        assert_eq!(list[0], "/p19");
    }
}
