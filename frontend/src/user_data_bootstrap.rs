//! 用户级数据：会话与偏好均经 **`/user-data`**（无 `localStorage` 回退）。

use crate::api::user_data::{
    fetch_current_web_sessions, fetch_user_data_prefs, put_user_data_prefs,
};
use crate::i18n::Locale;
use crate::storage::{ChatSession, normalize_workspace_partition_path};

/// 从服务端当前工作区桶加载侧栏会话。
pub async fn load_web_sessions(loc: Locale) -> (Vec<ChatSession>, Option<String>) {
    fetch_current_web_sessions(loc).await.unwrap_or_default()
}

/// 工作区 `POST /workspace` 成功后写入 `prefs.last_workspace_root`。
pub async fn persist_last_workspace_root(path: &str, loc: Locale) {
    let norm = normalize_workspace_partition_path(path);
    if norm.is_empty() {
        return;
    }
    let mut prefs = fetch_user_data_prefs(loc).await.unwrap_or_default();
    prefs.last_workspace_root = Some(norm);
    let _ = put_user_data_prefs(&prefs, loc).await;
}
