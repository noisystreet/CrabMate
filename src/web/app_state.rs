//! Web 服务进程内状态：会话存储、上传目录、任务队列句柄等（自 `lib.rs` 下沉）。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::chat_job_queue::ChatJobQueue;
use crate::config::AgentConfig;
use crate::types::{CommandApprovalDecision, Message};

/// 与 `normalize_client_conversation_id`（`chat_handlers`）及存储上限对齐。
pub(crate) const CONVERSATION_ID_MAX_LEN: usize = 128;
const CONVERSATION_STORE_MAX_ENTRIES: usize = 512;
const CONVERSATION_STORE_TTL: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone)]
pub(crate) struct ConversationEntry {
    messages: Vec<Message>,
    revision: u64,
    updated_at: std::time::Instant,
}

#[derive(Clone)]
pub(crate) struct ConversationTurnSeed {
    pub messages: Vec<Message>,
    pub expected_revision: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveConversationOutcome {
    Saved,
    Conflict,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) cfg: Arc<AgentConfig>,
    pub(crate) api_key: String,
    pub(crate) client: reqwest::Client,
    pub(crate) tools: Vec<crate::types::Tool>,
    /// 前端设置的工作区路径覆盖；为 None 时使用 cfg.run_command_working_dir
    pub(crate) workspace_override: Arc<tokio::sync::RwLock<Option<String>>>,
    pub(crate) uploads_dir: std::path::PathBuf,
    /// `/chat` / `/chat/stream` 进程内任务队列（有界排队 + 并发上限）
    pub(crate) chat_queue: ChatJobQueue,
    /// 基于 `conversation_id` 的进程内会话存储（PR-1：内存实现；后续可替换 Redis/DB）。
    pub(crate) conversation_store: Arc<tokio::sync::RwLock<HashMap<String, ConversationEntry>>>,
    /// 新会话 ID 递增计数器（仅用于生成默认 conversation_id）。
    pub(crate) conversation_id_counter: Arc<AtomicU64>,
    /// Web 流式审批会话 -> 决策通道。
    pub(crate) approval_sessions:
        Arc<tokio::sync::RwLock<HashMap<String, mpsc::Sender<CommandApprovalDecision>>>>,
}

impl AppState {
    pub(crate) fn web_api_auth_enabled(&self) -> bool {
        !self.cfg.web_api_bearer_token.trim().is_empty()
    }

    /// 当前生效的工作区根路径（前端已设置则用其值，否则用配置）
    pub(crate) async fn effective_workspace_path(&self) -> String {
        let guard = self.workspace_override.read().await;
        match guard.as_deref() {
            None => self.cfg.run_command_working_dir.clone(),
            Some(s) if s.trim().is_empty() => self.cfg.run_command_working_dir.clone(),
            Some(s) => s.to_string(),
        }
    }

    /// 前端是否已经“设置过工作区”（包含：显式选择默认目录）
    pub(crate) async fn workspace_is_set(&self) -> bool {
        let guard = self.workspace_override.read().await;
        guard.is_some()
    }

    pub(crate) fn next_conversation_id(&self) -> String {
        let n = self.conversation_id_counter.fetch_add(1, Ordering::Relaxed);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("conv_{}_{}", ts, n)
    }

    pub(crate) async fn load_conversation_seed(
        &self,
        conversation_id: &str,
    ) -> Option<ConversationTurnSeed> {
        let mut guard = self.conversation_store.write().await;
        let entry = guard.get_mut(conversation_id)?;
        if entry.updated_at.elapsed() > CONVERSATION_STORE_TTL {
            guard.remove(conversation_id);
            return None;
        }
        entry.updated_at = std::time::Instant::now();
        Some(ConversationTurnSeed {
            messages: entry.messages.clone(),
            expected_revision: Some(entry.revision),
        })
    }

    fn prune_conversation_store_locked(
        guard: &mut HashMap<String, ConversationEntry>,
        now: std::time::Instant,
    ) {
        guard.retain(|_, v| now.duration_since(v.updated_at) <= CONVERSATION_STORE_TTL);
        if guard.len() <= CONVERSATION_STORE_MAX_ENTRIES {
            return;
        }
        let mut order: Vec<(String, std::time::Instant)> = guard
            .iter()
            .map(|(k, v)| (k.clone(), v.updated_at))
            .collect();
        order.sort_by_key(|(_, t)| *t);
        let to_drop = guard.len() - CONVERSATION_STORE_MAX_ENTRIES;
        for (k, _) in order.into_iter().take(to_drop) {
            guard.remove(&k);
        }
    }

    pub(crate) async fn save_conversation_messages_if_revision(
        &self,
        conversation_id: String,
        messages: Vec<Message>,
        expected_revision: Option<u64>,
    ) -> SaveConversationOutcome {
        let mut guard = self.conversation_store.write().await;
        let now = std::time::Instant::now();
        if let Some(entry) = guard.get_mut(&conversation_id) {
            match expected_revision {
                Some(exp) if entry.revision == exp => {
                    entry.messages = messages;
                    entry.revision = entry.revision.saturating_add(1);
                    entry.updated_at = now;
                }
                _ => return SaveConversationOutcome::Conflict,
            }
        } else if expected_revision.is_some() {
            return SaveConversationOutcome::Conflict;
        } else {
            guard.insert(
                conversation_id,
                ConversationEntry {
                    messages,
                    revision: 1,
                    updated_at: now,
                },
            );
        }
        Self::prune_conversation_store_locked(&mut guard, now);
        SaveConversationOutcome::Saved
    }

    pub(crate) async fn conversation_count(&self) -> usize {
        self.conversation_store.read().await.len()
    }
}
