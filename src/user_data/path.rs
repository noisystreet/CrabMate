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

/// 与前端 `normalize_workspace_partition_path` 一致。
#[must_use]
pub fn normalize_workspace_partition_path(path: &str) -> String {
    path.trim().trim_end_matches('/').to_string()
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
}
