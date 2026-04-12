//! 聊天域聚合句柄：压缩 `App` → `chat_column_view` / `wire_chat_composer_streams` 的参数面，避免继续加长形参列表。
//!
//! 不引入 Leptos Context；仍为显式结构体传递，便于跳转与类型检查。根壳层另有 [`super::app_shell_ctx::AppShellCtx`]
//! 聚合侧栏 / 底栏 / 模态等 `*_view` 入参（同因 `Rc` 等未走 `provide_context`）。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use leptos::html::Textarea;
use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n::Locale;
use crate::sse_dispatch::ThinkingTraceInfo;

/// `/chat/stream` 与壳层共享的一组信号与句柄：状态栏错误、工具忙、审批、中止、工作区刷新、变更集拉取、澄清表单。
///
/// 从 [`WireComposerStreamsArgs`] 与 `composer_stream` 子模块的 **`ComposerStreamHandles`** 成组传入，
/// 避免 `composer_stream` 与 `App` 之间重复罗列同一批字段，并作为流式回调上下文的子聚合。
#[derive(Clone)]
pub struct ComposerStreamShell {
    pub status_busy: RwSignal<bool>,
    pub status_err: RwSignal<Option<String>>,
    pub tool_busy: RwSignal<bool>,
    pub pending_approval: RwSignal<Option<(String, String, String)>>,
    pub abort_cell: Arc<Mutex<Option<web_sys::AbortController>>>,
    pub user_cancelled_stream: Arc<Mutex<bool>>,
    pub refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    pub changelist_modal_open: RwSignal<bool>,
    pub changelist_fetch_nonce: RwSignal<u64>,
    pub pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    /// 服务端 `thinking_trace` 事件累积（新流开始时清空）。
    pub thinking_trace_log: RwSignal<Vec<ThinkingTraceInfo>>,
}

/// 中部聊天列：`messages` 滚动区、时间线、消息列表与输入区所需的信号与闭包。
///
/// 与 [`ComposerStreamShell`] 共享 **`status_busy` / `status_err` / `pending_clarification`** 等句柄，
/// 避免 `App` 在 `wire_chat_composer_streams` 与 `chat_column_view` 之间重复传入同一组 `RwSignal`。
#[derive(Clone)]
pub struct ChatColumnShell {
    pub locale: RwSignal<Locale>,
    pub messages_scroller: NodeRef<leptos::html::Div>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub messages_scroll_from_effect: RwSignal<bool>,
    pub last_messages_scroll_top: RwSignal<i32>,
    pub timeline_panel_expanded: RwSignal<bool>,
    pub chat: ChatSessionSignals,
    pub collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    pub expanded_tool_run_heads: RwSignal<HashSet<String>>,
    pub chat_find_query: RwSignal<String>,
    pub chat_find_match_ids: RwSignal<Vec<String>>,
    pub chat_find_cursor: RwSignal<usize>,
    pub composer_input_ref: NodeRef<Textarea>,
    pub composer_buf_ta: Arc<Mutex<String>>,
    pub pending_images: RwSignal<Vec<String>>,
    /// 与 `wire_chat_composer_streams` / SSE 回调共用（含 `status_busy`、`status_err`、`pending_clarification`）。
    pub stream_shell: ComposerStreamShell,
    pub run_send_message: Arc<dyn Fn() + Send + Sync>,
    pub trigger_stop: Arc<dyn Fn() + Send + Sync>,
    pub initialized: RwSignal<bool>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
}

/// `wire_chat_composer_streams` 的输入：流式、草稿、审批与工作区刷新等接线句柄。
pub struct WireComposerStreamsArgs {
    pub initialized: RwSignal<bool>,
    pub chat: ChatSessionSignals,
    pub locale: RwSignal<Locale>,
    pub draft: RwSignal<String>,
    pub selected_agent_role: RwSignal<Option<String>>,
    /// 与 SSE 流式回调共享的壳层状态（见 [`ComposerStreamShell`]）。
    pub stream_shell: ComposerStreamShell,
    pub composer_draft_buffer: Arc<Mutex<String>>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub pending_images: RwSignal<Vec<String>>,
}
