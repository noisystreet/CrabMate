//! 聊天域聚合句柄：压缩 `App` → `chat_column_view` / `wire_chat_composer_streams` 的参数面，避免继续加长形参列表。
//!
//! 不引入 Leptos Context；仍为显式结构体传递，便于跳转与类型检查。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use leptos::html::Textarea;
use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n::Locale;

/// 中部聊天列：`messages` 滚动区、时间线、消息列表与输入区所需的信号与闭包。
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
    pub pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    pub run_send_message: Arc<dyn Fn() + Send + Sync>,
    pub trigger_stop: Arc<dyn Fn() + Send + Sync>,
    pub status_busy: RwSignal<bool>,
    pub initialized: RwSignal<bool>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub status_err: RwSignal<Option<String>>,
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
    pub status_busy: RwSignal<bool>,
    pub status_err: RwSignal<Option<String>>,
    pub pending_approval: RwSignal<Option<(String, String, String)>>,
    pub tool_busy: RwSignal<bool>,
    pub composer_draft_buffer: Arc<Mutex<String>>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub abort_cell: Arc<Mutex<Option<web_sys::AbortController>>>,
    pub user_cancelled_stream: Arc<Mutex<bool>>,
    pub refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    pub changelist_modal_open: RwSignal<bool>,
    pub changelist_fetch_nonce: RwSignal<u64>,
    pub pending_images: RwSignal<Vec<String>>,
    pub pending_clarification: RwSignal<Option<PendingClarificationForm>>,
}
