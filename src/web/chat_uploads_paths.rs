//! 聊天附图落盘：与会话 SQLite **同目录**（`conversations.db` 旁的 `chat_uploads/`）。
//! 进程启动时选定后**不**随 `POST /workspace` 切换（会话库本身也不跟工作区走）。

use std::path::{Path, PathBuf};

use crate::web::app_state::AppStateHttpCore;

/// `<workspace>/.crabmate/chat_uploads`（无会话库路径时的回退）。
#[must_use]
pub(crate) fn chat_uploads_dir_for_workspace(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".crabmate").join("chat_uploads")
}

/// 与 `conversation_store_sqlite_path` 同级的 `chat_uploads/`；空路径则回退工作区 `.crabmate`。
#[must_use]
pub(crate) fn chat_uploads_dir_beside_session_store(
    sqlite_path: &str,
    fallback_workspace: &Path,
) -> PathBuf {
    let t = sqlite_path.trim();
    if !t.is_empty() {
        let p = Path::new(t);
        if let Some(parent) = p.parent() {
            if parent.as_os_str().is_empty() {
                return PathBuf::from("chat_uploads");
            }
            return parent.join("chat_uploads");
        }
    }
    chat_uploads_dir_for_workspace(fallback_workspace)
}

/// 工作区切换：只更新出站 `@` 图用的工作区根，**不**改附图落盘目录。
pub(crate) async fn sync_chat_runtime_paths_for_workspace(
    http: &AppStateHttpCore,
    workspace_root: &Path,
) {
    let mut g = http.cfg.write().await;
    g.chat_workspace_root = Some(workspace_root.to_path_buf());
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uploads_dir_is_sibling_of_conversations_db() {
        let dir = chat_uploads_dir_beside_session_store(
            "/tmp/proj/.crabmate/conversations.db",
            Path::new("/other/ws"),
        );
        assert_eq!(dir, PathBuf::from("/tmp/proj/.crabmate/chat_uploads"));
        let fb = chat_uploads_dir_beside_session_store("", Path::new("/ws"));
        assert_eq!(fb, PathBuf::from("/ws/.crabmate/chat_uploads"));
    }
}
