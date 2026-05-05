//! 侧栏与会话列表的展示顺序：置顶 > 收藏 > `updated_at` 降序（同刻按 `id` 稳定）。

use crate::storage::ChatSession;

/// 就地排序，供侧栏「最近」与管理会话模态等共用。
pub fn sort_sessions_for_display(sessions: &mut [ChatSession]) {
    sessions.sort_by(|a, b| match (a.pinned, b.pinned) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => match (a.starred, b.starred) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => match b.updated_at.cmp(&a.updated_at) {
                std::cmp::Ordering::Equal => a.id.cmp(&b.id),
                o => o,
            },
        },
    });
}

/// 返回排序后的副本（不修改 `sessions` 向量本身顺序，仅用于展示）。
pub fn sorted_sessions_clone(sessions: &[ChatSession]) -> Vec<ChatSession> {
    let mut v = sessions.to_vec();
    sort_sessions_for_display(&mut v);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: &str, updated: i64, pinned: bool, starred: bool) -> ChatSession {
        ChatSession {
            id: id.to_string(),
            title: id.to_string(),
            draft: String::new(),
            messages: vec![],
            updated_at: updated,
            pinned,
            starred,
            server_conversation_id: None,
            server_revision: None,
        }
    }

    #[test]
    fn pinned_before_starred_before_time() {
        let mut v = vec![
            s("a", 100, false, false),
            s("b", 200, false, true),
            s("c", 50, true, false),
            s("d", 300, false, false),
        ];
        sort_sessions_for_display(&mut v);
        assert_eq!(v[0].id, "c");
        assert_eq!(v[1].id, "b");
        assert_eq!(v[2].id, "d");
        assert_eq!(v[3].id, "a");
    }
}
