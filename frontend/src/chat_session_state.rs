//! 会话列表、当前会话、后端流式 job 与水合 nonce 的**信号聚合**。
//!
//! # 写入关系（简表）
//!
//! - **`sessions` / `active_id`**：侧栏、作曲器、持久化 `Effect`、工具/导出等；流式 delta 通过 [`Self::stream_bound_session_id`]（与 attach 时快照一致）写入对应会话，**不一定**等于当时的 [`Self::active_id`]。
//! - **`stream_job_id` / `stream_last_event_seq`**：SSE 首包与 `id:` 行；应用 [`ChatSessionSignals::clear_stream_resume_handles`] 表示「放弃当前断线重连上下文」（错误、结束、`stream_ended`、会话切换等）。
//! - **`stream_bound_session_id`**：与 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::active_session_id`] 同源——**发起 attach 时**的快照；可与 UI [`Self::active_id`] 对比以发现「侧栏已切会话但 SSE 仍写旧会话」。
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
    /// 当前（或刚结束）`/chat/stream` 写入的目标会话 id；与闭包内 [`ChatStreamCallbackCtx::active_session_id`] 一致。
    ///
    /// `None` 表示无进行中的流式绑定（或已调用 [`Self::clear_stream_resume_handles`]）。
    pub stream_bound_session_id: RwSignal<Option<String>>,
    /// 流式 SSE 累积的 `reasoning_text`（服务端不存），hydration 覆盖后从此恢复。
    pub reasoning_preserved: RwSignal<HashMap<String, String>>,
}

impl ChatSessionSignals {
    /// 记录本轮 attach 时 SSE 回调应写入的会话（须与 [`crate::app::chat::composer_stream::make_attach_chat_stream`] 内 `ChatStreamCallbackCtx` 使用同一字符串）。
    #[inline]
    pub fn bind_stream_to_session(self, session_id: String) {
        self.stream_bound_session_id.set(Some(session_id));
    }

    /// 清空断线重连用的服务端流句柄（响应头 `x-stream-job-id` 与 SSE `id:` 序号）。
    ///
    /// 与「重置 `sessions` 向量」无关；会话切换、流结束、致命错误等路径应调用此处而非散落两处 `set`。
    /// 同时清空 [`Self::stream_bound_session_id`]，与「无进行中的流式写会话」语义一致。
    pub fn clear_stream_resume_handles(self) {
        self.stream_job_id.set(None);
        self.stream_last_event_seq.set(0);
        self.stream_bound_session_id.set(None);
    }
}
