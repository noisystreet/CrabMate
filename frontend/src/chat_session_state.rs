//! 会话列表、当前会话、后端流式 job 与水合 nonce 的**信号聚合**。
//!
//! # 写入关系（简表）
//!
//! - **`sessions` / `active_id`**：侧栏、作曲器、持久化 `Effect`、工具/导出等；流式助手正文增量默认进 [`Self::stream_text_overlay`]，收尾时合并回会话，**不一定**每 token 触发 `sessions` 无效化。
//! - **`stream_bound_session_id`**：与 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::bound_stream_session_id`] 同源（**发起 attach 时**快照），决定 SSE 写哪条会话，**不一定**等于当时的 [`Self::active_id`]。
//! - **`stream_job_id` / `stream_last_event_seq`**：SSE 首包与 `id:` 行；应用 [`ChatSessionSignals::clear_stream_resume_handles`] 表示「放弃当前断线重连上下文」（错误、结束、`stream_ended`、会话切换等）。
//! - **`session_sync`**：服务端 `conversation_id` / revision，与 `POST /chat/branch` 等对齐。
//! - **`session_hydrate_nonce` / `reasoning_preserved`**：拉取会话正文与水合时的补偿字段。
//! - **`stream_text_overlay`**：尾条 `loading` 助手消息的流式正文/思维链旁路缓冲（字段见 [`ChatSessionSignals`]）。
//!
//! # `sessions` 向量的写入通道（命名封装）
//!
//! 底层仍是 [`RwSignal::update`]；下列方法仅用于**标明语义来源**，便于检索竞态（水合 nonce、流绑定 id、分支 revision 等），并非运行时强制分区。
//!
//! | 方法 | 典型调用方 |
//! |------|-----------|
//! | [`Self::update_sessions_hydration`] | `app::session_hydrate` |
//! | [`Self::update_sessions_composer`] | `app::chat::composer_wires`、`session_ops::patch_active_session`（经 `sessions`） |
//! | [`Self::update_sessions_stream_sse`] | `composer_stream::callbacks::stream_session_access` |
//! | [`Self::update_sessions_branch`] | `chat_actions`、`POST /chat/branch` 成功后 revision |
//! | [`Self::update_sessions_message_row`] | `message_row_actions`（再生/分支截断） |

use std::collections::HashMap;

use leptos::prelude::*;

use crate::session_sync::SessionSyncState;
use crate::storage::ChatSession;
use crate::stream_text_overlay::StreamTextOverlay;

/// 是否存在仍处于 Loading 的工具时间线气泡（与 SSE `tool_running` / `tool_busy` 互补，避免状态栏已「就绪」但卡片仍在转圈）。
///
/// 会话 id 取 [`Self::effective_stream_message_session_id`]（与进行中 SSE 写入目标一致，无流时回落 UI 当前会话）。
pub fn session_has_loading_tool_message(chat: ChatSessionSignals) -> bool {
    let sid = chat.effective_stream_message_session_id();
    chat.sessions.with(|sessions| {
        sessions.iter().any(|s| {
            s.id == sid
                && s.messages
                    .iter()
                    .any(|m| m.is_tool && m.state.as_ref().is_some_and(|st| st.is_loading()))
        })
    })
}

/// Web 流式：`tool_busy` ∨ 时间线 Loading 工具占位（状态栏「工具执行中」与此对齐）。
///
/// 与 [`ChatStreamBusyMemos::stream_turn_busy_ui`] 组合即为「整回合 UI 忙」，规则集中在一处构造，避免多处手写相同 OR。
#[derive(Clone, Copy)]
pub struct ChatStreamBusyMemos {
    pub stream_turn_busy_ui: Memo<bool>,
    pub tool_timeline_busy_ui: Memo<bool>,
}

/// 在 [`crate::app::chat::wire_chat_domain::wire_chat_domain_effects`] 内**单次**构造，经 [`crate::app::chat::handles::ChatColumnShell`] 下发到底栏 / 作曲器 / 消息行。
#[must_use]
pub fn make_chat_stream_busy_memos(
    chat: ChatSessionSignals,
    status_busy: RwSignal<bool>,
    tool_busy: RwSignal<bool>,
) -> ChatStreamBusyMemos {
    let tool_timeline_busy_ui =
        Memo::new(move |_| tool_busy.get() || session_has_loading_tool_message(chat));
    let stream_turn_busy_ui = Memo::new(move |_| status_busy.get() || tool_timeline_busy_ui.get());
    ChatStreamBusyMemos {
        stream_turn_busy_ui,
        tool_timeline_busy_ui,
    }
}

/// 与单会话聊天、流式 `/chat/stream`、服务端快照对齐相关的响应式句柄。
#[derive(Clone, Copy)]
pub struct ChatSessionSignals {
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub session_sync: RwSignal<SessionSyncState>,
    pub session_hydrate_nonce: RwSignal<u64>,
    pub stream_job_id: RwSignal<Option<u64>>,
    pub stream_last_event_seq: RwSignal<u64>,
    /// 当前（或刚结束）`/chat/stream` 写入的目标会话 id；与闭包内 [`ChatStreamCallbackCtx::bound_stream_session_id`] 一致。
    ///
    /// `None` 表示无进行中的流式绑定（或已调用 [`Self::clear_stream_resume_handles`]）。
    pub stream_bound_session_id: RwSignal<Option<String>>,
    /// 流式 SSE 累积的 `reasoning_text`（服务端不存），hydration 覆盖后从此恢复。
    pub reasoning_preserved: RwSignal<HashMap<String, String>>,
    /// 当前尾条 `loading` 助手消息的流式正文/思维链增量；**不**写入 [`Self::sessions`]，减少历史行重算。
    pub stream_text_overlay: RwSignal<Option<StreamTextOverlay>>,
}

impl ChatSessionSignals {
    /// 工具时间线 / 「哪条会话上有 loading 工具」等：有在途流时与 [`Self::stream_bound_session_id`] 一致，否则与侧栏 [`Self::active_id`] 一致。
    #[must_use]
    pub fn effective_stream_message_session_id(self) -> String {
        self.stream_bound_session_id
            .get()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.active_id.get())
    }

    /// 记录本轮 attach 时 SSE 回调应写入的会话（须与 [`crate::app::chat::composer_stream::make_attach_chat_stream`] 内 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::bound_stream_session_id`] 使用同一字符串）。
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

    /// [`GET /conversation/messages`] 水合合并与 reasoning 恢复。
    #[inline]
    pub fn update_sessions_hydration(self, f: impl FnOnce(&mut Vec<ChatSession>)) {
        self.sessions.update(f);
    }

    /// 作曲器：发送、重试、取消流、新建会话等。
    #[inline]
    pub fn update_sessions_composer(self, f: impl FnOnce(&mut Vec<ChatSession>)) {
        self.sessions.update(f);
    }

    /// SSE 流式回调（须与 [`Self::stream_bound_session_id`] / attach 快照一致）。
    #[inline]
    pub fn update_sessions_stream_sse(self, f: impl FnOnce(&mut Vec<ChatSession>)) {
        self.sessions.update(f);
    }

    /// `POST /chat/branch` 成功后对齐 `server_revision` 等。
    #[inline]
    pub fn update_sessions_branch(self, f: impl FnOnce(&mut Vec<ChatSession>)) {
        self.sessions.update(f);
    }

    /// 消息行：再生、分支、本地截断。
    #[inline]
    pub fn update_sessions_message_row(self, f: impl FnOnce(&mut Vec<ChatSession>)) {
        self.sessions.update(f);
    }
}
