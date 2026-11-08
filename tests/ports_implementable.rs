// 端口都在 `server` 面（`cm_agent` / `cm_approval` / `cm_session_store` 均挂该 feature）。
// 没有这行，`cargo check --no-default-features --all-targets` 会编译本文件并全体失败。
#![cfg(feature = "server")]

//! S6-d（#1298）：端口 trait 必须能从 crate 外**实现**——这是「承诺面成立」的唯一硬证据。
//!
//! 本文件是**集成测试**（外部 crate 视角）：只能用 `crabmate::` 根上导出的名字。
//! 一旦某个端口被收回 `pub(crate)`，或它签名闭包里的类型不再可达，这里就会编译失败——
//! 这正是 `docs/design/sdk_facade.md` §7.2 想要的效果，而不是靠清单里的一行文字。
//!
//! 关于 [`crabmate::async_trait`]：re-export 只保证**版本对齐**，不能免除依赖——
//! `#[async_trait]` 的展开会引用 `::async_trait`，下游仍需自己声明该依赖。本测试与
//! crate 同包（可选依赖已激活），故可直接用 re-export 的宏名。

use std::collections::HashSet;
use std::sync::Arc;

use crabmate::async_trait;
use crabmate::cm_agent::permission_audit::PermissionAxis;
use crabmate::cm_types::{ApprovalPolicy, PathScope, PermissionLevel};
use crabmate::types::Message;
use crabmate::{
    AgentTurnSubPhase, ApprovalBroker, BranchTruncateOutcome, ConversationRetention,
    ConversationTurnSeed, PolicyEngine, RunAgentTurnError, SaveConversationOutcome, SessionStore,
    TurnEngine, TurnOutcome, TurnRequest, WebApprovalSink,
};

/// 外部策略：只回答「放行一切」。用来证明端口没有隐藏的必填能力。
struct AllowAll;

impl PolicyEngine for AllowAll {
    fn level(&self) -> PermissionLevel {
        PermissionLevel::Read
    }

    fn approval(&self) -> ApprovalPolicy {
        ApprovalPolicy::Gate
    }

    fn path_scope(&self) -> PathScope {
        PathScope::SessionRoot
    }

    fn outbound_denied(&self) -> bool {
        false
    }

    fn outbound_allows_any(&self) -> bool {
        true
    }

    fn outbound_prefixes(&self) -> &[String] {
        &[]
    }

    fn command_allowlist(&self) -> Option<Arc<[String]>> {
        None
    }

    fn denied_axis(&self, _name: &str) -> Option<PermissionAxis> {
        None
    }

    fn deny_message(&self, name: &str) -> String {
        format!("denied: {name}")
    }
}

/// 外部审批通道：无 SSE 交互（运维 CLI / 无头路径的形态）。
struct NullBroker {
    out_tx: tokio::sync::mpsc::Sender<String>,
    /// 持有接收端：丢掉它 `Sender::is_closed()` 立刻为真，断言就失去意义。
    _out_rx: tokio::sync::mpsc::Receiver<String>,
    allowlist: Arc<tokio::sync::Mutex<HashSet<String>>>,
}

impl NullBroker {
    fn new() -> Self {
        let (out_tx, out_rx) = tokio::sync::mpsc::channel(1);
        Self {
            out_tx,
            _out_rx: out_rx,
            allowlist: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
        }
    }
}

impl ApprovalBroker for NullBroker {
    fn approval_sink(&self) -> Option<WebApprovalSink<'_>> {
        None
    }

    fn turn_job_id(&self) -> Option<u64> {
        None
    }

    fn out_tx(&self) -> &tokio::sync::mpsc::Sender<String> {
        &self.out_tx
    }

    fn persistent_allowlist(&self) -> &Arc<tokio::sync::Mutex<HashSet<String>>> {
        &self.allowlist
    }
}

/// 外部回合引擎：不真跑模型，只证明「可实现 + 可命名错误类型」。
struct FailingEngine;

#[async_trait]
impl TurnEngine for FailingEngine {
    async fn run(&self, _params: TurnRequest<'_>) -> Result<TurnOutcome, RunAgentTurnError> {
        Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Executor,
            message: "外部实现（本测试不跑回合）".to_string(),
        })
    }
}

/// 外部存储：内存空实现。九个方法都返回「空」语义。
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

    #[allow(clippy::too_many_arguments)]
    async fn save_if_revision(
        &self,
        _conversation_id: String,
        _messages: Vec<Message>,
        _active_agent_role: Option<&str>,
        _active_session_mode: Option<&str>,
        _active_permission_level: Option<&str>,
        _expected_revision: Option<u64>,
        _archive_rows: Vec<crabmate::ArchiveRow>,
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
fn policy_engine_is_implementable_from_outside() {
    let policy: &dyn PolicyEngine = &AllowAll;
    assert!(policy.allows_tool("read_file"));
    assert!(!policy.outbound_uses_prefixes());
    assert_eq!(policy.deny_message("x"), "denied: x");
}

#[test]
fn approval_broker_is_implementable_from_outside() {
    let broker = NullBroker::new();
    let port: &dyn ApprovalBroker = &broker;
    assert!(port.approval_sink().is_none());
    assert!(port.turn_job_id().is_none());
    assert!(!port.out_tx().is_closed());
    assert!(port.persistent_allowlist().try_lock().is_ok());
}

#[test]
fn turn_engine_is_implementable_from_outside() {
    let engine: Arc<dyn TurnEngine> = Arc::new(FailingEngine);
    assert_eq!(Arc::strong_count(&engine), 1);
}

#[test]
fn session_store_is_implementable_from_outside() {
    let store: &dyn SessionStore = &EmptyStore;
    assert!(!store.is_sqlite());
    // 端口签名里的伴生类型必须外部可构造（否则「可实现」只是纸面结论）。
    let retention = ConversationRetention {
        ttl_secs: 0,
        max_entries: 0,
        archive_max_rows_per_conversation: 0,
        archive_codec_min_bytes: 0,
    };
    assert_eq!(retention.max_entries, 0);
    let seed = ConversationTurnSeed {
        messages: Vec::new(),
        expected_revision: None,
        persisted_active_agent_role: None,
        persisted_active_session_mode: None,
        persisted_active_permission_level: None,
        layout: None,
    };
    assert!(seed.messages.is_empty());
}
