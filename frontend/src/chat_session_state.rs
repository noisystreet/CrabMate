//! 会话列表、当前会话、后端流式 job 与水合 nonce 的**信号聚合**。
//!
//! # 写入关系（简表）
//!
//! - **`sessions` / `active_id`**：侧栏、作曲器、持久化 `Effect`、工具/导出等；流式 delta 只应改**当前**会话的 `messages`。
//! - **`stream_job_id` / `stream_last_event_seq`**：SSE 首包与 `id:` 行；应用 [`ChatSessionSignals::clear_stream_resume_handles`] 表示「放弃当前断线重连上下文」（错误、结束、`stream_ended`、会话切换等）。
//! - **`session_sync`**：服务端 `conversation_id` / revision，与 `POST /chat/branch` 等对齐。
//! - **`session_hydrate_nonce` / `reasoning_preserved`**：拉取会话正文与水合时的补偿字段。

use std::collections::HashMap;

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
    /// 流式 SSE 累积的 `reasoning_text`（服务端不存），hydration 覆盖后从此恢复。
    pub reasoning_preserved: RwSignal<HashMap<String, String>>,
}

impl ChatSessionSignals {
    /// 清空断线重连用的服务端流句柄（响应头 `x-stream-job-id` 与 SSE `id:` 序号）。
    ///
    /// 与「重置 `sessions` 向量」无关；会话切换、流结束、致命错误等路径应调用此处而非散落两处 `set`。
    pub fn clear_stream_resume_handles(self) {
        self.stream_job_id.set(None);
        self.stream_last_event_seq.set(0);
    }
}
