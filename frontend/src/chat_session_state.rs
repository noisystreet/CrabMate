//! 会话列表、当前会话、后端流式 job 与水合 nonce 的**信号聚合**。
//!
//! # 写入关系（简表）
//!
//! - **`sessions` / `active_id`**：侧栏、合成器、持久化 `Effect`、工具/导出等；流式助手正文增量默认进 [`Self::stream_text_overlay`]，收尾时合并回会话，**不一定**每 token 触发 `sessions` 无效化。
//! - **`stream_bound_session_id`**：与 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::bound_stream_session_id`] 同源（**发起 attach 时**快照），决定 SSE 写哪条会话，**不一定**等于当时的 [`Self::active_id`]。
//! - **`stream_job_id` / `stream_last_event_seq`**：SSE 首包与 `id:` 行；应用 [`ChatSessionSignals::clear_stream_resume_handles`] 表示「放弃当前断线重连上下文」（错误、结束、`stream_ended`、会话切换等）。上述四槽位亦经 [`ChatStreamSessionLane`] / [`ChatSessionSignals::stream_session_lane`] 成组访问。
//! - **`stream_attach_generation`**：每次发起新 `/chat/stream` attach 递增；[`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx`] 捕获代际，回调内若与当前值不一致则视为陈旧（上一轮 `abort` 后仍可能排队执行），避免写全局句柄/旁路缓冲。
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
use std::sync::Arc;

use leptos::prelude::*;

use crate::session_sync::SessionSyncState;
use crate::storage::ChatSession;
use crate::stream_text_overlay::StreamTextOverlay;

/// 与单轮 `/chat/stream` 绑定的 **RwSignal 槽位**（`job_id` / SSE 序号 / 绑定会话 / 尾条 overlay）。
///
/// 与 [`ChatSessionSignals`] 上平铺字段同源；用于把「流式侧」多槽位收拢为单处传递/解构，减少
/// `stream_bound_session_id` / `stream_job_id` / `stream_text_overlay` 分散读写的心智负担。
#[derive(Clone, Copy, Debug)]
pub struct ChatStreamSessionLane {
    pub bound_session_id: RwSignal<Option<String>>,
    pub job_id: RwSignal<Option<u64>>,
    pub last_event_seq: RwSignal<u64>,
    pub text_overlay: RwSignal<Option<StreamTextOverlay>>,
}

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

/// 当前流式目标会话是否存在仍处于 `Loading` 的助手或工具占位（订阅 `sessions`）。
#[must_use]
pub fn session_has_stream_loading_placeholders(chat: ChatSessionSignals) -> bool {
    let sid = chat.effective_stream_message_session_id();
    chat.sessions.with(|sessions| {
        sessions.iter().find(|s| s.id == sid).is_some_and(|s| {
            s.messages.iter().any(|m| {
                let row = matches!(
                    (m.role.as_str(), m.is_tool),
                    ("assistant", false) | (_, true)
                );
                m.state.as_ref().is_some_and(|st| st.is_loading()) && row
            })
        })
    })
}

/// 在 `sid` 会话中是否存在**与即将发起的 `/chat/stream` attach**相冲突的 Loading：
/// 任意工具 Loading，或 **非** `except_plain_assistant_id` 的普通助手 Loading。
///
/// 用于「截断后再生」路径：`truncate_at_user_message_and_prepare_regenerate` 会先插入一条 **Loading**
/// 尾条助手；若此处仍用 [`session_has_stream_loading_placeholders`] 与 `status_busy` 做 OR，
/// 会把自身当成「整轮忙」而**永远**无法触发 `attach`。
#[must_use]
pub(crate) fn session_has_conflicting_stream_loading_in_messages(
    sessions: &[ChatSession],
    sid: &str,
    except_plain_assistant_id: &str,
) -> bool {
    let Some(s) = sessions.iter().find(|sess| sess.id == sid) else {
        return false;
    };
    s.messages.iter().any(|m| {
        if !m.state.as_ref().is_some_and(|st| st.is_loading()) {
            return false;
        }
        if m.is_tool {
            return true;
        }
        m.role == "assistant" && m.id != except_plain_assistant_id
    })
}

/// [`session_has_conflicting_stream_loading_in_messages`] 的会话信号封装（订阅 `sessions`）。
#[must_use]
pub fn session_has_conflicting_stream_loading_placeholders(
    chat: ChatSessionSignals,
    except_plain_assistant_id: &str,
) -> bool {
    let sid = chat.effective_stream_message_session_id();
    chat.sessions.with(|sessions| {
        session_has_conflicting_stream_loading_in_messages(
            sessions,
            &sid,
            except_plain_assistant_id,
        )
    })
}

/// 当前流式目标会话是否存在仍处于 `Loading` 的助手或工具占位（**不**订阅 `sessions`）。
#[must_use]
pub(crate) fn session_has_stream_loading_placeholders_untracked(chat: ChatSessionSignals) -> bool {
    let sid = chat.effective_stream_message_session_id_untracked();
    chat.sessions.with_untracked(|sessions| {
        sessions.iter().find(|s| s.id == sid).is_some_and(|s| {
            s.messages.iter().any(|m| {
                let row = matches!(
                    (m.role.as_str(), m.is_tool),
                    ("assistant", false) | (_, true)
                );
                m.state.as_ref().is_some_and(|st| st.is_loading()) && row
            })
        })
    })
}

/// Web 流式：`tool_busy` ∨ 时间线 Loading 工具占位（状态栏「工具执行中」与此对齐）。
///
/// **`stream_turn_busy_ui`** 另见 [`make_chat_stream_busy_memos`]：与「停止」门闩同源，不再手写第二套 OR。
#[derive(Clone, Copy)]
pub struct ChatStreamBusyMemos {
    pub stream_turn_busy_ui: Memo<bool>,
    pub tool_timeline_busy_ui: Memo<bool>,
}

/// 在 [`crate::app::chat::wire_chat_domain::wire_chat_domain_effects`] 内**单次**构造，经 [`crate::app::chat::handles::ChatColumnShell`] 下发到底栏 / 合成器 / 消息行。
///
/// **`stream_turn_busy_ui`**：`status_busy` ∨ 工具时间线忙 ∨ 助手 Loading 占位 ∨ **`AbortController` 槽位已占用**，
/// 与 [`crate::app::chat::stream_user_abort::stream_ui_inflight_untracked`] 使用同一套 OR，避免「停止」门闩与忙状态分裂。
#[must_use]
pub fn make_chat_stream_busy_memos(
    chat: ChatSessionSignals,
    status_busy: RwSignal<bool>,
    tool_busy: RwSignal<bool>,
    stream_abort_epoch: RwSignal<u32>,
    abort_present: Arc<dyn Fn() -> bool + Send + Sync>,
) -> ChatStreamBusyMemos {
    let tool_timeline_busy_ui =
        Memo::new(move |_| tool_busy.get() || session_has_loading_tool_message(chat));
    let ap = Arc::clone(&abort_present);
    let stream_turn_busy_ui = Memo::new(move |_| {
        let _ = stream_abort_epoch.get();
        status_busy.get()
            || tool_busy.get()
            || session_has_stream_loading_placeholders(chat)
            || ap()
    });
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
    /// 每次发起新 `/chat/stream` attach 时递增，与 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::attach_generation`] 比对以丢弃陈旧 SSE 回调。
    pub stream_attach_generation: RwSignal<u64>,
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
    /// 流式写入通道：与 [`Self::stream_bound_session_id`] 等字段一一对应，便于热路径单参传递。
    #[must_use]
    pub const fn stream_session_lane(self) -> ChatStreamSessionLane {
        ChatStreamSessionLane {
            bound_session_id: self.stream_bound_session_id,
            job_id: self.stream_job_id,
            last_event_seq: self.stream_last_event_seq,
            text_overlay: self.stream_text_overlay,
        }
    }

    /// 工具时间线 / 「哪条会话上有 loading 工具」等：有在途流时与 [`Self::stream_bound_session_id`] 一致，否则与侧栏 [`Self::active_id`] 一致。
    #[must_use]
    pub fn effective_stream_message_session_id(self) -> String {
        self.stream_bound_session_id
            .get()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.active_id.get())
    }

    /// 与 [`Self::effective_stream_message_session_id`] 相同规则，用于「停止」等不得订阅 UI 的快照路径。
    #[must_use]
    pub fn effective_stream_message_session_id_untracked(self) -> String {
        self.stream_bound_session_id
            .get_untracked()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.active_id.get_untracked())
    }

    /// 记录本轮 attach 时 SSE 回调应写入的会话（须与 [`crate::app::chat::composer_stream::make_attach_chat_stream`] 内 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::bound_stream_session_id`] 使用同一字符串）。
    #[inline]
    pub fn bind_stream_to_session(self, session_id: String) {
        self.stream_bound_session_id.set(Some(session_id));
    }

    /// 发起新一轮流式 attach 时调用，返回**本轮**代际值（写入 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::attach_generation`]）。
    #[inline]
    pub fn bump_stream_attach_generation(self) -> u64 {
        let next = self
            .stream_attach_generation
            .get_untracked()
            .wrapping_add(1);
        self.stream_attach_generation.set(next);
        next
    }

    /// 清空断线重连用的服务端流句柄（响应头 `x-stream-job-id` 与 SSE `id:` 序号）。
    ///
    /// 与「重置 `sessions` 向量」无关；会话切换、流结束、致命错误等路径应调用此处而非散落两处 `set`。
    /// 同时清空 [`Self::stream_bound_session_id`]，与「无进行中的流式写会话」语义一致。
    pub fn clear_stream_resume_handles(self) {
        let lane = self.stream_session_lane();
        lane.job_id.set(None);
        lane.last_event_seq.set(0);
        lane.bound_session_id.set(None);
    }

    /// [`GET /conversation/messages`] 水合合并与 reasoning 恢复。
    #[inline]
    pub fn update_sessions_hydration(self, f: impl FnOnce(&mut Vec<ChatSession>)) {
        self.sessions.update(f);
    }

    /// 合成器：发送、重试、取消流、新建会话等。
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

#[cfg(test)]
mod conflict_loading_tests {
    use super::session_has_conflicting_stream_loading_in_messages;
    use crate::storage::{ChatSession, StoredMessage, StoredMessageState};

    fn plain_assistant(id: &str, state: Option<StoredMessageState>) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: "assistant".to_string(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    fn loading_tool(id: &str) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: "assistant".to_string(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: true,
            tool_call_id: Some("tc1".into()),
            tool_name: Some("read_file".into()),
            created_at: 0,
        }
    }

    #[test]
    fn except_assistant_loading_does_not_conflict() {
        let sid = "s1";
        let keep = "asst_new";
        let sessions = vec![ChatSession {
            id: sid.to_string(),
            title: String::new(),
            draft: String::new(),
            messages: vec![plain_assistant(keep, Some(StoredMessageState::Loading))],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
        }];
        assert!(!session_has_conflicting_stream_loading_in_messages(
            &sessions, sid, keep
        ));
    }

    #[test]
    fn other_assistant_loading_conflicts() {
        let sid = "s1";
        let sessions = vec![ChatSession {
            id: sid.to_string(),
            title: String::new(),
            draft: String::new(),
            messages: vec![
                plain_assistant("old", Some(StoredMessageState::Loading)),
                plain_assistant("new", None),
            ],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
        }];
        assert!(session_has_conflicting_stream_loading_in_messages(
            &sessions, sid, "new"
        ));
    }

    #[test]
    fn loading_tool_conflicts_even_when_except_matches_assistant() {
        let sid = "s1";
        let sessions = vec![ChatSession {
            id: sid.to_string(),
            title: String::new(),
            draft: String::new(),
            messages: vec![
                loading_tool("t1"),
                plain_assistant("asst_new", Some(StoredMessageState::Loading)),
            ],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
        }];
        assert!(session_has_conflicting_stream_loading_in_messages(
            &sessions, sid, "asst_new"
        ));
    }
}
