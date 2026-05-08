//! 聊天域聚合句柄：压缩 `App` → `chat_column_view` / `wire_chat_composer_streams` 的参数面，避免继续加长形参列表。
//!
//! 不引入 Leptos Context；仍为显式结构体传递，便于跳转与类型检查。根壳层另有 [`super::app_shell_ctx::AppShellCtx`]
//! 聚合侧栏 / 底栏 / 模态等 `*_view` 入参（同因 `Rc` 等未走 `provide_context`）。

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;

use crate::app::app_signals::{AppSignals, StreamControlSignals};
use crate::chat_session_state::{ChatSessionSignals, ChatStreamBusyMemos};
use crate::clarification_form::PendingClarificationForm;
use crate::i18n::Locale;
use crate::sse_dispatch::ThinkingTraceInfo;

/// 流式路径使用的审批 / 澄清 / 思维迹信号子集（与 [`AppSignals::approval`] 同源句柄）。
#[derive(Clone, Copy)]
pub struct ComposerStreamApprovalSignals {
    pub pending_approval: RwSignal<Option<(String, String, String)>>,
    pub pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    /// 服务端 `thinking_trace` 事件累积（新流开始时清空）。
    pub thinking_trace_log: RwSignal<Vec<ThinkingTraceInfo>>,
}

/// 流式与工作区刷新联动的变更集模态信号子集（与 [`AppSignals::modal`] 同源句柄）。
#[derive(Clone, Copy)]
pub struct ComposerStreamModalSignals {
    pub changelist_modal_open: RwSignal<bool>,
    pub changelist_fetch_nonce: RwSignal<u64>,
}

/// `/chat/stream` 与壳层共享的一组信号与句柄：状态栏错误、工具忙、审批、中止、工作区刷新、变更集拉取、澄清表单。
///
/// 字段按 **`stream` / `approval` / `modal`** 与 `AppSignals` 子域对齐，并通过 [`ComposerStreamShell::from_app_signals`] **单点组装**，
/// 避免在壳初始化处手写逐字段拷贝导致漏接。
///
/// 从 [`WireComposerStreamsArgs`] 与 `composer_stream` 子模块的 **`ComposerStreamHandles`** 成组传入，
/// 避免 `composer_stream` 与 `App` 之间重复罗列同一批字段，并作为流式回调上下文的子聚合。
#[derive(Clone)]
pub struct ComposerStreamShell {
    pub stream: StreamControlSignals,
    pub approval: ComposerStreamApprovalSignals,
    pub modal: ComposerStreamModalSignals,
    pub refresh_workspace: Arc<dyn Fn() + Send + Sync>,
}

impl ComposerStreamShell {
    /// 从 [`AppSignals`] 抽取流式所需句柄并附带工作区刷新闭包（与原先 `app_shell_init` 手写赋值语义一致）。
    #[must_use]
    pub fn from_app_signals(
        app: &AppSignals,
        refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            stream: app.stream.clone(),
            approval: ComposerStreamApprovalSignals {
                pending_approval: app.approval.pending_approval,
                pending_clarification: app.approval.pending_clarification,
                thinking_trace_log: app.approval.thinking_trace_log,
            },
            modal: ComposerStreamModalSignals {
                changelist_modal_open: app.modal.changelist_modal_open,
                changelist_fetch_nonce: app.modal.changelist_fetch_nonce,
            },
            refresh_workspace,
        }
    }
}

/// 中部聊天列：`messages` 滚动区、时间线、消息列表与输入区所需的信号与闭包。
///
/// **不再**逐字段复制 [`AppSignals::chat_composer`] / [`AppSignals::shell_ui`]：视图从 [`Self::app`] 读取，
/// 仅保留流式子壳与 `wire_chat_composer_streams` 产出的闭包 / 信号，避免与根壳层双重映射。
///
/// 与 [`ComposerStreamShell`] 共享 **`status_busy` / `status_err` / `pending_clarification`** 等句柄。
#[derive(Clone)]
pub struct ChatColumnShell {
    pub app: AppSignals,
    pub stream_shell: ComposerStreamShell,
    /// 流式回合「UI 忙」派生（[`ChatStreamBusyMemos`]）；与 SSE 原始 busy 信号同源接线，避免视图层重复拼规则。
    pub stream_busy_memos: ChatStreamBusyMemos,
    pub run_send_message: Arc<dyn Fn() + Send + Sync>,
    pub trigger_stop: Arc<dyn Fn() + Send + Sync>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub retry_assistant_target: RwSignal<Option<String>>,
}

/// `wire_chat_composer_streams` 的返回值：重试 / 截断再生目标与发送、停止、新会话句柄。
pub(crate) struct ChatComposerWires {
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub run_send_message: Arc<dyn Fn() + Send + Sync>,
    pub cancel_stream: Arc<dyn Fn() + Send + Sync>,
    pub new_session: Rc<dyn Fn()>,
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
    /// 见 [`ChatStreamBusyMemos::stream_turn_busy_ui`]。
    pub stream_turn_busy_ui: Memo<bool>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub pending_images: RwSignal<Vec<String>>,
}
