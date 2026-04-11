//! 会话列表、当前会话、后端流式 job 与水合 nonce 的**信号聚合**。
//!
//! 将原先在 `App` 与各 `wire_*` 之间逐项传递的 `RwSignal` 收拢为一处，降低参数爆炸与错序传递风险；
//! 不包含业务逻辑（水合、SSE 仍放在对应模块）。

use leptos::prelude::*;

use crate::session_sync::SessionSyncState;
use crate::storage::ChatSession;

/// 与单会话聊天、流式 `/chat/stream`、服务端快照对齐相关的响应式句柄。
#[derive(Clone, Copy)]
pub struct ChatSessionSignals {
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub session_sync: RwSignal<SessionSyncState>,
    pub session_hydrate_nonce: RwSignal<u64>,
    pub stream_job_id: RwSignal<Option<u64>>,
    pub stream_last_event_seq: RwSignal<u64>,
}
