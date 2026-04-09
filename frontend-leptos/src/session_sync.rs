//! 浏览器标签页内「本地 `ChatSession`」与后端持久化会话（`conversation_id` + `conversation_saved.revision`）的对齐模型。
//!
//! 单一聚合状态由 [`SessionSyncState`] 表示，避免 `conversation_id` / `conversation_revision` 两个 `RwSignal` 隐含组合语义分散在各处。

/// 当前活跃本地会话相对服务端存储的关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionPersistence {
    /// 仅本地 `localStorage` 消息；无服务端 `conversation_id`（或已显式清除绑定）。
    #[default]
    LocalOnly,
    /// 已收到 `x-conversation-id`（或等价 SSE），后续轮次应携带该 id；`revision` 见 [`SessionSyncState::last_known_revision`]。
    ServerBound,
}

/// 与 `POST /chat/stream`、`POST /chat/branch`、`GET /workspace/changelog` 相关的会话同步快照。
#[derive(Debug, Clone)]
pub struct SessionSyncState {
    pub persistence: SessionPersistence,
    /// 服务端会话 id（响应头 `x-conversation-id`）；本地独有会话为 `None`。
    pub conversation_id: Option<String>,
    /// 最近一次 SSE `conversation_saved.revision`；首轮保存事件前为 `None`。
    pub last_known_revision: Option<u64>,
    /// `POST /chat/branch` 因 revision 不一致等原因失败后置位，直至下一次成功 branch 或新的 `conversation_saved`。
    pub branch_conflict: bool,
}

/// 供 UI / 调试展示的粗粒度阶段（由 [`SessionSyncState::sync_kind`] 导出）。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSyncKind {
    /// 与 [`SessionPersistence::LocalOnly`] 一致。
    LocalOnly,
    /// 已绑定 `conversation_id`，尚未收到 `revision`（例如首轮流式尚未落盘）。
    ServerAwaitingRevision,
    /// 已绑定且具备 `last_known_revision`，可对服务端做 `branch`（在配置启用存储时）。
    ServerSynced,
    /// Branch 失败后的失步状态；回退为仅本地截断/重试直至重新对齐。
    ServerRevisionStale,
}

impl SessionSyncState {
    pub fn local_only() -> Self {
        Self {
            persistence: SessionPersistence::LocalOnly,
            conversation_id: None,
            last_known_revision: None,
            branch_conflict: false,
        }
    }

    #[allow(dead_code)]
    pub fn sync_kind(&self) -> SessionSyncKind {
        match self.persistence {
            SessionPersistence::LocalOnly => SessionSyncKind::LocalOnly,
            SessionPersistence::ServerBound => {
                if self.branch_conflict {
                    SessionSyncKind::ServerRevisionStale
                } else if self.last_known_revision.is_some() {
                    SessionSyncKind::ServerSynced
                } else {
                    SessionSyncKind::ServerAwaitingRevision
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn reset_to_local(&mut self) {
        *self = Self::local_only();
    }

    /// 新的流式回合收到 `x-conversation-id` 时调用：绑定服务端会话并清空旧 revision。
    pub fn apply_stream_conversation_id(&mut self, id: String) {
        self.persistence = SessionPersistence::ServerBound;
        self.conversation_id = Some(id);
        self.last_known_revision = None;
        self.branch_conflict = false;
    }

    /// SSE `conversation_saved.revision`。
    pub fn apply_saved_revision(&mut self, rev: u64) {
        if self.persistence == SessionPersistence::ServerBound {
            self.last_known_revision = Some(rev);
            self.branch_conflict = false;
        }
    }

    pub fn set_revision_after_branch(&mut self, rev: u64) {
        self.last_known_revision = Some(rev);
        self.branch_conflict = false;
    }

    pub fn mark_branch_conflict(&mut self) {
        self.branch_conflict = true;
        self.last_known_revision = None;
    }

    /// `GET /workspace/changelog?conversation_id=`（有则带 scope）。
    pub fn changelog_conversation_id(&self) -> Option<&str> {
        self.conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// `POST /chat/stream` 请求体中的 `conversation_id`。
    pub fn stream_conversation_id(&self) -> Option<String> {
        self.conversation_id.clone()
    }

    /// 用户消息「重试 / 分支」：若返回 `(Some(id), Some(rev))` 则先 `POST /chat/branch`。
    pub fn branch_id_and_expected_revision(&self) -> (Option<&str>, Option<u64>) {
        let id = self
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let rev = if self.branch_conflict {
            None
        } else {
            self.last_known_revision
        };
        (id, rev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_only_kind() {
        let s = SessionSyncState::local_only();
        assert_eq!(s.sync_kind(), SessionSyncKind::LocalOnly);
    }

    #[test]
    fn server_bound_awaits_revision() {
        let mut s = SessionSyncState::local_only();
        s.apply_stream_conversation_id("c1".into());
        assert_eq!(s.sync_kind(), SessionSyncKind::ServerAwaitingRevision);
    }

    #[test]
    fn saved_revision_synced() {
        let mut s = SessionSyncState::local_only();
        s.apply_stream_conversation_id("c1".into());
        s.apply_saved_revision(3);
        assert_eq!(s.sync_kind(), SessionSyncKind::ServerSynced);
    }

    #[test]
    fn branch_conflict_stale() {
        let mut s = SessionSyncState::local_only();
        s.apply_stream_conversation_id("c1".into());
        s.apply_saved_revision(3);
        s.mark_branch_conflict();
        assert_eq!(s.sync_kind(), SessionSyncKind::ServerRevisionStale);
        assert_eq!(s.branch_id_and_expected_revision(), (Some("c1"), None));
    }
}
