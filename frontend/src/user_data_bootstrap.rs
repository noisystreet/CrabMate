//! 用户级数据：会话与偏好均经 **`/user-data`**（无 `localStorage` 回退）。

use leptos::prelude::*;

use crate::api::user_data::{UserPrefsDto, fetch_current_web_sessions};
use crate::i18n::Locale;
use crate::storage::{ChatSession, normalize_workspace_partition_path};

/// 与后端 `RECENT_WORKSPACE_ROOTS_MAX` 一致。
pub const RECENT_WORKSPACE_ROOTS_MAX: usize = 10;

/// 从服务端当前工作区桶加载侧栏会话。
pub async fn load_web_sessions(loc: Locale) -> (Vec<ChatSession>, Option<String>) {
    fetch_current_web_sessions(loc).await.unwrap_or_default()
}

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

/// 从 prefs 得到最近列表：优先 `recent_workspace_roots`；旧数据仅有 `last_workspace_root` 时回填一项。
#[must_use]
pub fn recent_roots_from_prefs(dto: &UserPrefsDto) -> Vec<String> {
    let mut list = Vec::new();
    for p in &dto.recent_workspace_roots {
        push_recent_workspace_root(&mut list, p);
    }
    if list.is_empty() {
        if let Some(ref last) = dto.last_workspace_root {
            push_recent_workspace_root(&mut list, last);
        }
    }
    list
}

/// 菜单展示用：取路径末段；完整路径作 `title`。
#[must_use]
pub fn workspace_recent_menu_label(path: &str) -> String {
    let p = path.trim().trim_end_matches(['/', '\\']);
    let name = p
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(p);
    if name.is_empty() {
        path.to_string()
    } else {
        name.to_string()
    }
}

/// 工作区 `POST /workspace` 成功后更新内存中的最近列表；落盘由 `user_prefs_sync` 防抖 PUT。
pub fn remember_workspace_root(path: &str, recent: RwSignal<Vec<String>>) {
    let norm = normalize_workspace_partition_path(path);
    if norm.is_empty() {
        return;
    }
    recent.update(|list| {
        push_recent_workspace_root(list, &norm);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_recent_dedupes_and_caps() {
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

    #[test]
    fn recent_roots_from_prefs_falls_back_to_last() {
        let dto = UserPrefsDto {
            last_workspace_root: Some("/old/ws/".into()),
            recent_workspace_roots: Vec::new(),
            ..UserPrefsDto::default()
        };
        assert_eq!(recent_roots_from_prefs(&dto), vec!["/old/ws".to_string()]);
    }

    #[test]
    fn menu_label_takes_last_segment() {
        assert_eq!(workspace_recent_menu_label("/home/me/proj"), "proj");
        assert_eq!(workspace_recent_menu_label(r"C:\Users\me\proj\"), "proj");
    }
}
