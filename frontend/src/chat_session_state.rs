//! 会话列表、当前会话、后端流式 job 与水合 nonce 的**信号聚合**。
//!
//! # 写入关系（简表）
//!
//! - **`sessions` / `active_id`**：侧栏、合成器、持久化 `Effect`、工具/导出等；流式助手正文增量默认进 [`Self::stream_text_overlay`]，收尾时合并回会话，**不一定**每 token 触发 `sessions` 无效化。
//! - **`stream_transport`**：[`ChatStreamTransport`] 单 **`RwSignal`** 收敛 **attach 代际** + **Idle / 已绑定会话**（内含 **`job_id`**）；[`ChatSessionSignals::clear_stream_resume_handles`] 将车道置回 Idle 并重置 [`Self::stream_last_sse_event_seq`]；[`ChatSessionSignals::bind_stream_to_session`] 在 bump 后写入 Bound。SSE `id:` 序号单独为 **`stream_last_sse_event_seq`**，避免热路径上整包传输状态无效化。与 **`stream_text_overlay`** 的联合象限见 [`StreamLaneOverlayPhase`] / [`ChatSessionSignals::stream_lane_overlay_phase_untracked`]。
//! - **代际门闩**：每次发起新 `/chat/stream` attach 时递增 [`ChatStreamTransport::attach_generation`]；[`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx`] 捕获该值，回调内若与当前不一致则视为陈旧。
//! - **`session_sync`**：服务端 `conversation_id` / revision，与 `POST /chat/branch` 等对齐。
//! - **`session_hydrate_nonce` / `reasoning_preserved` / `conversation_prompt_tokens`**：拉取会话正文与水合时的补偿字段；后者供底栏展示 prompt token 粗估。
//! - **`stream_text_overlay`**：尾条 `loading` 助手消息的流式正文/思维链旁路缓冲（字段见 [`ChatSessionSignals`]）。**只读展示**须经 [`crate::stream_text_overlay::message_text_for_display_including_stream_overlay`]（`parent_session_id` 为该消息所在会话 id），勿仅对 `StoredMessage` 调 [`crate::message_format::message_text_for_display_ex`]。
//!
//! # `sessions` 向量的写入通道（命名封装）
//!
//! 底层仍是 [`RwSignal::update`]；下列方法仅用于**标明语义来源**，便于检索竞态（水合 nonce、流绑定 id、分支 revision 等），并非运行时强制分区。
//!
//! | 方法 | 典型调用方 |
//! |------|-----------|
//! | [`Self::update_sessions_hydration`] | `app::chat::session_hydrate` |
//! | [`Self::update_sessions_composer`] | `app::chat::composer_wires`、`session_ops::patch_active_session`（经 `sessions`） |
//! | [`Self::update_sessions_stream_sse`] | `composer_stream::callbacks::stream_session_access` |
//! | [`Self::update_sessions_branch`] | `chat_actions`、`POST /chat/branch` 成功后 revision |
//! | [`Self::update_sessions_message_row`] | `message_row_actions`（再生/分支截断） |

use std::collections::HashMap;
use std::sync::Arc;

use leptos::prelude::*;

use crate::conversation_hydrate::TiktokenPromptTokensSnapshot;
use crate::session_sync::SessionSyncState;
use crate::storage::ChatSession;
use crate::stream_text_overlay::StreamTextOverlay;

/// 水合成功后与会话绑定的 tiktoken prompt 粗估（见 `GET /conversation/messages`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationPromptTokenHydrate {
    pub conversation_id: String,
    pub tiktoken: Option<TiktokenPromptTokensSnapshot>,
}

/// `/chat/stream` 传输层：单调 **`attach_generation`** 与 **`lane`**（Idle | 已绑定会话及重连句柄）。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ChatStreamTransport {
    pub attach_generation: u64,
    pub lane: ChatStreamTransportLane,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ChatStreamTransportLane {
    /// 无进行中的流式写绑定（或已 [`ChatSessionSignals::clear_stream_resume_handles`]）。
    #[default]
    Idle,
    /// 已 [`ChatSessionSignals::bind_stream_to_session`]；SSE 回调应写入 `session_id`。
    Bound {
        session_id: String,
        job_id: Option<u64>,
    },
}

impl ChatStreamTransport {
    #[must_use]
    pub fn bound_session_id(&self) -> Option<&str> {
        match &self.lane {
            ChatStreamTransportLane::Idle => None,
            ChatStreamTransportLane::Bound { session_id, .. } => Some(session_id.as_str()),
        }
    }

    fn bump_attach_generation(&mut self) -> u64 {
        self.attach_generation = self.attach_generation.wrapping_add(1);
        self.attach_generation
    }

    fn bind_stream_session(&mut self, session_id: String) {
        self.lane = ChatStreamTransportLane::Bound {
            session_id,
            job_id: None,
        };
    }

    fn clear_resume_handles(&mut self) {
        self.lane = ChatStreamTransportLane::Idle;
    }

    pub(crate) fn set_stream_job_id(&mut self, jid: u64) {
        if let ChatStreamTransportLane::Bound { job_id, .. } = &mut self.lane {
            *job_id = Some(jid);
        }
    }
}

/// [`ChatStreamTransportLane`] 与尾条 [`StreamTextOverlay`] 的联合象限（只读快照；调试与水合守卫用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamLaneOverlayPhase {
    IdleClear,
    IdleWithOverlay,
    BoundClear,
    BoundWithOverlay,
}

/// 与单轮 `/chat/stream` 尾条 overlay 相关的 **`RwSignal`**（热路径仅 bump overlay，不克隆整块 [`ChatStreamTransport`]）。
#[derive(Clone, Copy, Debug)]
pub struct ChatStreamSessionLane {
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

/// 在 `sid` 会话中是否存在**与即将发起的 `/chat/stream` attach**相冲突的 Loading：
/// 任意工具 Loading，或 **非** `except_plain_assistant_id` 的普通助手 Loading。
///
/// 用于「截断后再生」路径：`truncate_at_user_message_and_prepare_regenerate` 会先插入一条 **Loading**
/// 尾条助手；须排除该 id，否则会把自身当成冲突而永远无法 `attach`。
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

/// Web 流式：`tool_timeline_busy_ui` 见 [`make_chat_stream_busy_memos`]（[`crate::app::chat::turn_lifecycle`] + Loading 工具占位）。
///
/// **`stream_turn_busy_ui`** 与「停止」门闩同源，由 [`crate::app::chat::turn_lifecycle`] 驱动。
#[derive(Clone, Copy)]
pub struct ChatStreamBusyMemos {
    pub stream_turn_busy_ui: Memo<bool>,
    pub tool_timeline_busy_ui: Memo<bool>,
    /// 底栏「模型生成中」指示（Attaching / Draining / 非工具 Streaming）。
    pub model_status_busy: Memo<bool>,
}

/// 在 [`crate::app::chat::wire_chat_domain::wire_chat_domain_effects`] 内**单次**构造，经 [`crate::app::chat::handles::ChatColumnShell`] 下发到底栏 / 合成器 / 消息行。
///
/// **`stream_turn_busy_ui`**：[`TurnLifecycle`] 粗 busy ∨ **`AbortController` 槽位**；
/// 与 [`crate::app::chat::stream_user_abort::stream_ui_inflight_untracked`] 使用同一套谓词。
/// Loading 占位须在 `on_done` / 中止 / 错误路径于 `messages` 上收口（见 `composer_stream/callbacks/done_session`）。
#[must_use]
pub fn make_chat_stream_busy_memos(
    chat: ChatSessionSignals,
    turn_lifecycle: RwSignal<crate::app::turn_lifecycle::TurnLifecycleState>,
    stream_abort_epoch: RwSignal<u32>,
    abort_present: Arc<dyn Fn() -> bool + Send + Sync>,
) -> ChatStreamBusyMemos {
    use crate::app::turn_lifecycle::{
        turn_lifecycle_model_ui_busy, turn_lifecycle_stream_turn_busy, turn_lifecycle_tool_ui_busy,
    };

    let tool_timeline_busy_ui = Memo::new(move |_| {
        turn_lifecycle_tool_ui_busy(turn_lifecycle.get()) || session_has_loading_tool_message(chat)
    });
    let model_status_busy = Memo::new(move |_| turn_lifecycle_model_ui_busy(turn_lifecycle.get()));
    let ap = Arc::clone(&abort_present);
    let stream_turn_busy_ui = Memo::new(move |_| {
        let _ = stream_abort_epoch.get();
        turn_lifecycle_stream_turn_busy(turn_lifecycle.get(), ap())
    });
    ChatStreamBusyMemos {
        stream_turn_busy_ui,
        tool_timeline_busy_ui,
        model_status_busy,
    }
}

/// 与单会话聊天、流式 `/chat/stream`、服务端快照对齐相关的响应式句柄。
#[derive(Clone, Copy)]
pub struct ChatSessionSignals {
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub session_sync: RwSignal<SessionSyncState>,
    pub session_hydrate_nonce: RwSignal<u64>,
    /// 流式 attach 代际、绑定会话与断线重连句柄（[`ChatStreamTransport`]）。
    pub stream_transport: RwSignal<ChatStreamTransport>,
    /// SSE `id:` 行最后序号（与 `stream_transport` 解耦，避免热路径整包克隆）。
    pub stream_last_sse_event_seq: RwSignal<u64>,
    /// 流式 SSE 累积的 `reasoning_text`（服务端不存），hydration 覆盖后从此恢复。
    pub reasoning_preserved: RwSignal<HashMap<String, String>>,
    /// 当前尾条 `loading` 助手消息的流式正文/思维链增量；**不**写入 [`Self::sessions`]，减少历史行重算。
    pub stream_text_overlay: RwSignal<Option<StreamTextOverlay>>,
    /// 当前应订阅 [`Self::stream_text_overlay`] 热路径的助手 `message_id`（仅该行 Markdown 组件订阅，避免 Tauri/WebView 主线程假卡死）。
    pub stream_overlay_display_mid: RwSignal<Option<String>>,
    /// overlay 内容变更计数（滚动跟底 / 持久化防抖等轻量订阅，勿直接 `.get()` 整包 [`Self::stream_text_overlay`]）。
    pub stream_overlay_revision: RwSignal<u64>,
    /// 工具输出流式累积（tool_call_id → 文本），避免每 chunk 写 sessions。
    pub tool_output_chunks: RwSignal<HashMap<String, String>>,
    /// 最近一次成功水合的 tiktoken prompt 计数（与 [`ConversationPromptTokenHydrate::conversation_id`] 对齐，防串会话）。
    pub conversation_prompt_tokens: RwSignal<Option<ConversationPromptTokenHydrate>>,
    /// 正在拉取更早一页历史（`GET /conversation/messages?before_index=`）。
    pub history_loading_older: RwSignal<bool>,
}

impl ChatSessionSignals {
    /// 清空流式 overlay 与展示订阅目标（`on_done` / 用户中止 / 新 attach 前）。
    #[inline]
    pub fn clear_stream_text_overlay(self) {
        self.stream_text_overlay.set(None);
        self.stream_overlay_display_mid.set(None);
    }

    /// 同步 overlay 展示订阅目标（尾泡轮换 / 工具后续写时须与 [`StreamTextOverlay::message_id`] 一致）。
    #[inline]
    pub fn set_stream_overlay_display_mid(self, message_id: &str) {
        if message_id.is_empty() {
            self.stream_overlay_display_mid.set(None);
        } else {
            self.stream_overlay_display_mid
                .set(Some(message_id.to_string()));
        }
    }

    /// 流式或 overlay 未收尾时不应拉取服务端快照（避免水合与 SSE 写回竞态）。
    #[must_use]
    pub fn defers_conversation_hydration_untracked(self) -> bool {
        if self.stream_text_overlay.get_untracked().is_some() {
            return true;
        }
        self.stream_transport
            .get_untracked()
            .bound_session_id()
            .is_some()
    }
}

impl ChatSessionSignals {
    /// 流式 overlay 句柄（[`Self::stream_text_overlay`]）；传输层见 [`Self::stream_transport`]。
    #[must_use]
    pub const fn stream_session_lane(self) -> ChatStreamSessionLane {
        ChatStreamSessionLane {
            text_overlay: self.stream_text_overlay,
        }
    }

    /// [`ChatStreamTransportLane`] 与 [`Self::stream_text_overlay`] 的联合象限（**不**订阅 UI）。
    #[must_use]
    pub fn stream_lane_overlay_phase_untracked(self) -> StreamLaneOverlayPhase {
        let transport = self.stream_transport.get_untracked();
        let overlay_present = self.stream_text_overlay.get_untracked().is_some();
        match &transport.lane {
            ChatStreamTransportLane::Idle => {
                if overlay_present {
                    StreamLaneOverlayPhase::IdleWithOverlay
                } else {
                    StreamLaneOverlayPhase::IdleClear
                }
            }
            ChatStreamTransportLane::Bound { .. } => {
                if overlay_present {
                    StreamLaneOverlayPhase::BoundWithOverlay
                } else {
                    StreamLaneOverlayPhase::BoundClear
                }
            }
        }
    }

    /// 工具时间线 / 「哪条会话上有 loading 工具」等：有在途流时与 Bound 会话 id 一致，否则与侧栏 [`Self::active_id`] 一致。
    #[must_use]
    pub fn effective_stream_message_session_id(self) -> String {
        self.stream_transport
            .get()
            .bound_session_id()
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| self.active_id.get())
    }

    /// 当前全局 attach 代际（**不**订阅 UI）；与 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::attach_generation`] 比对。
    #[must_use]
    pub fn stream_attach_generation_untracked(self) -> u64 {
        self.stream_transport.get_untracked().attach_generation
    }

    /// 记录本轮 attach 时 SSE 回调应写入的会话（须与 [`crate::app::chat::composer_stream::make_attach_chat_stream`] 内 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::bound_stream_session_id`] 使用同一字符串）。
    ///
    /// `attach_generation` **须**为同轮 [`Self::bump_stream_attach_generation`] 的返回值。
    #[inline]
    pub fn bind_stream_to_session(self, session_id: String, attach_generation: u64) {
        #[cfg(not(debug_assertions))]
        let _ = attach_generation;
        self.stream_transport.update(|t| {
            #[cfg(debug_assertions)]
            debug_assert_eq!(
                t.attach_generation, attach_generation,
                "bind_stream_to_session: attach_generation must match bump_stream_attach_generation"
            );
            t.bind_stream_session(session_id);
        });
    }

    /// 发起新一轮流式 attach 时调用，返回**本轮**代际值（写入 [`crate::app::chat::composer_stream::context::ChatStreamCallbackCtx::attach_generation`]）。
    #[inline]
    pub fn bump_stream_attach_generation(self) -> u64 {
        let mut out = 0u64;
        self.stream_transport.update(|t| {
            out = t.bump_attach_generation();
        });
        out
    }

    /// 清空断线重连用的服务端流句柄（响应头 `x-stream-job-id` 与 SSE `id:` 序号）。
    ///
    /// 与「重置 `sessions` 向量」无关；会话切换、流结束、致命错误等路径应调用此处。
    /// 将传输车道置为 Idle（**不**回退 [`ChatStreamTransport::attach_generation`]）。
    pub fn clear_stream_resume_handles(self) {
        self.stream_transport
            .update(ChatStreamTransport::clear_resume_handles);
        self.stream_last_sse_event_seq.set(0);
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

    /// SSE 流式回调（须与 [`Self::stream_transport`] Bound 会话 / attach 快照一致）。
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
            history_total: None,
            history_window_start: None,
            history_has_older: None,
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
            history_total: None,
            history_window_start: None,
            history_has_older: None,
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
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        }];
        assert!(session_has_conflicting_stream_loading_in_messages(
            &sessions, sid, "asst_new"
        ));
    }
}
