//! 会话存储**端口**与内存 / SQLite 实现（S1-b，设计见 `docs/design/sdk_facade.md` §5.3）。
//!
//! 位置：本模块挂在 **`sqlite`** feature 之下、`web` **之上**——它的消费者既有 Web
//! （`AppState`），也有运维 CLI（`cli_run_serve`）。搬出 `web/` 之前，`cli_run_serve.rs`
//! 直接写着 `web::ConversationBacking`，运维 CLI 反向依赖 Web 层；现在两边都只认本模块。
//!
//! **端口面**：[`SessionStore`] 是执行面实际消费的那 9 个方法，[`ConversationBacking`]
//! （内存 / SQLite 两态枚举）是当前唯一实现。端口只收 owned 入参 + 已存在的公开 DTO
//! （`ConversationRetention` / `SaveConversationOutcome` / `BranchTruncateOutcome` /
//! `ArchiveRow` / [`ConversationTurnSeed`]），不新增语义。
//!
//! **判定规则没有第二套**：`backing.rs` 的 `impl` 就是原 `web/conversation_backing.rs`
//! 的实现逐字搬移（日志文案、回退口径、`spawn_blocking` + 加锁顺序均未改）。

use std::collections::HashSet;

use async_trait::async_trait;

use crate::cm_api_contract::chat::ConversationLayoutMeta;
use crate::conversation_store::{ArchiveRow, BranchTruncateOutcome, SaveConversationOutcome};
use crate::types::Message;

mod backing;

pub(crate) use backing::{ConversationBacking, ConversationRetention, open_conversation_sqlite};

/// 读会话种子：`messages` + revision + 三列 `active_*` + layout。
#[derive(Clone)]
pub(crate) struct ConversationTurnSeed {
    pub messages: Vec<Message>,
    pub expected_revision: Option<u64>,
    pub persisted_active_agent_role: Option<String>,
    /// 持久化的会话工作模式（ask/plan/act）；`None` 表示未设置。
    pub persisted_active_session_mode: Option<String>,
    /// 持久化的权限等级**意图**（chat/…/unrestricted）；`None` 表示未设置。
    pub persisted_active_permission_level: Option<String>,
    /// 可选布局元数据；缺省时 GET 省略。会话级，不随消息分页切片。
    pub layout: Option<ConversationLayoutMeta>,
}

/// 会话存储端口：执行面只需要这 9 个能力（S1-b）。
///
/// 入参一律 owned / `&str`，不借 `web` 的类型：运维 CLI、`save-session` 与未来的终端
/// 可以直接用同一个端口，而不必把 Web 层拖进来。
#[async_trait]
pub(crate) trait SessionStore: Send + Sync {
    /// 当前后端是否 SQLite（少数只在 SQLite 下成立的语义用它分流，例如归档）。
    fn is_sqlite(&self) -> bool;

    /// 按策略清理过期 / 超量会话。
    async fn prune(&self, r: ConversationRetention);

    /// 读会话种子（`None` = 不存在 / 已过期）。
    async fn load_seed(&self, conversation_id: &str, ttl_secs: u64)
    -> Option<ConversationTurnSeed>;

    /// 读会话归档原文（仅 SQLite 有；内存后端返回空）。
    async fn load_archive_messages(&self, conversation_id: &str) -> Vec<Message>;

    /// 整行覆盖 + CAS（`expected_revision`）+ 归档落盘 + 保留策略回收。
    ///
    /// 形参多与实现一致：这些值同属一次落盘事务，拆结构体只会把「一排 None」变成
    /// 「一排字段 = None」（同 `AppStateConversationRuntime` 该方法的注释）。
    #[allow(clippy::too_many_arguments)]
    async fn save_if_revision(
        &self,
        conversation_id: String,
        messages: Vec<Message>,
        active_agent_role: Option<&str>,
        active_session_mode: Option<&str>,
        active_permission_level: Option<&str>,
        expected_revision: Option<u64>,
        archive_rows: Vec<ArchiveRow>,
        r: ConversationRetention,
    ) -> SaveConversationOutcome;

    /// 分叉截断到第 `user_ordinal` 条普通用户消息之前（CAS）。
    async fn truncate_before_user_ordinal_if_revision(
        &self,
        conversation_id: String,
        user_ordinal: usize,
        expected_revision: u64,
        r: ConversationRetention,
    ) -> BranchTruncateOutcome;

    /// 只改会话级 `active_*` 意图列。
    async fn set_active_state(
        &self,
        conversation_id: String,
        role: Option<&str>,
        mode: Option<&str>,
        level: Option<&str>,
    ) -> Result<bool, String>;

    /// 会话正文里仍引用的 `/uploads/<filename>`（清理附图时跳过）。
    async fn referenced_upload_filenames(&self) -> HashSet<String>;

    /// 持久化会话条数。
    async fn conversation_count(&self) -> usize;

    /// 删除持久化会话行；**幂等**（不存在时静默成功）。
    async fn delete(&self, conversation_id: &str);
}

/// 从会话 JSON / 消息正文收集 `/uploads/<filename>` 引用。
pub(crate) fn collect_upload_filenames_from_text(s: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut rest = s;
    while let Some(i) = rest.find("/uploads/") {
        rest = &rest[i + "/uploads/".len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            .collect();
        if !name.is_empty() && !name.contains("..") {
            out.insert(name.clone());
        }
        rest = rest.get(name.len()..).unwrap_or("");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_upload_names_from_session_json() {
        let s = r#"{"url":"/uploads/u1_2_3.png","x":"/uploads/../x"}"#;
        let names = collect_upload_filenames_from_text(s);
        assert!(names.contains("u1_2_3.png"));
        assert!(!names.iter().any(|n| n.contains("..")));
    }

    /// 端口可替换性：一个**非** `ConversationBacking` 的实现在执行面上等价可用。
    ///
    /// 等价性由「同一份实现逐字搬移」保证；这条测试钉的是**端口本身**——
    /// 若哪天有人把 `&dyn SessionStore` 改回具体 enum，编译期才会发现。
    struct EmptyStore;

    #[async_trait]
    impl SessionStore for EmptyStore {
        fn is_sqlite(&self) -> bool {
            false
        }

        async fn prune(&self, _r: ConversationRetention) {}

        async fn load_seed(&self, _id: &str, _ttl: u64) -> Option<ConversationTurnSeed> {
            None
        }

        async fn load_archive_messages(&self, _id: &str) -> Vec<Message> {
            Vec::new()
        }

        async fn save_if_revision(
            &self,
            _conversation_id: String,
            _messages: Vec<Message>,
            _active_agent_role: Option<&str>,
            _active_session_mode: Option<&str>,
            _active_permission_level: Option<&str>,
            _expected_revision: Option<u64>,
            _archive_rows: Vec<ArchiveRow>,
            _r: ConversationRetention,
        ) -> SaveConversationOutcome {
            SaveConversationOutcome::Saved
        }

        async fn truncate_before_user_ordinal_if_revision(
            &self,
            _conversation_id: String,
            _user_ordinal: usize,
            _expected_revision: u64,
            _r: ConversationRetention,
        ) -> BranchTruncateOutcome {
            BranchTruncateOutcome::Saved
        }

        async fn set_active_state(
            &self,
            _conversation_id: String,
            _role: Option<&str>,
            _mode: Option<&str>,
            _level: Option<&str>,
        ) -> Result<bool, String> {
            Ok(false)
        }

        async fn referenced_upload_filenames(&self) -> HashSet<String> {
            HashSet::new()
        }

        async fn conversation_count(&self) -> usize {
            0
        }

        async fn delete(&self, _conversation_id: &str) {}
    }

    #[test]
    fn session_store_is_object_safe_and_substitutable() {
        let store: &dyn SessionStore = &EmptyStore;
        assert!(!store.is_sqlite());
        fn assert_bounds(_: &(dyn SessionStore + Send + Sync)) {}
        assert_bounds(&EmptyStore);
    }
}
