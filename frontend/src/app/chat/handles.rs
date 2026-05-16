//! 聊天域聚合句柄：压缩 `App` → `chat_column_view` / `wire_chat_composer_streams` 的参数面，避免继续加长形参列表。
//!
//! 不引入整份壳层 Leptos Context；[`super::shell_runtime_context::ChatShellLeptosContext`] 承载可 `Copy` 的聊天切片。
//! 仍为显式结构体传递 [`super::app_shell_ctx::AppShellCtx`]，便于跳转与类型检查（同因 `Rc` 等未走整包 `provide_context`）。

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;

use super::composer_follow_up::ComposerStreamFollowUp;
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

impl ComposerStreamApprovalSignals {
    /// 发起新一轮 `/chat/stream` 前：清空「待审批 / 待澄清」，避免与流式壳层状态打架。
    #[inline]
    pub(crate) fn clear_pending_user_interactions(self) {
        self.pending_approval.set(None);
        self.pending_clarification.set(None);
    }

    /// 收到 SSE 审批请求：与澄清表单**互斥**（后到的覆盖先到的语义由调用方保证顺序）。
    #[inline]
    pub(crate) fn replace_with_pending_approval(self, triple: (String, String, String)) {
        self.pending_clarification.set(None);
        self.pending_approval.set(Some(triple));
    }

    /// 收到 SSE 澄清问卷：与待审批**互斥**。
    #[inline]
    pub(crate) fn replace_with_pending_clarification(self, form: PendingClarificationForm) {
        self.pending_approval.set(None);
        self.pending_clarification.set(Some(form));
    }
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
/// 从 [`WireComposerStreamsArgs`]（[`WireComposerStreamsSessionSlice`] + [`WireComposerStreamsStreamSlice`]）与
/// `composer_stream` 子模块的 **`ComposerStreamHandles`** 成组传入，避免 `composer_stream` 与 `App` 之间重复罗列同一批字段。
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
    /// 截断再生 / 失败助手重试：由 [`super::composer_wires::wire_chat_composer_streams`] 单 Effect 消费。
    pub stream_follow_up: RwSignal<ComposerStreamFollowUp>,
}

/// `wire_chat_composer_streams` 的返回值：待发流式后续动作与发送、停止、新会话句柄。
pub(crate) struct ChatComposerWires {
    pub stream_follow_up: RwSignal<ComposerStreamFollowUp>,
    pub run_send_message: Arc<dyn Fn() + Send + Sync>,
    pub cancel_stream: Arc<dyn Fn() + Send + Sync>,
    pub new_session: Rc<dyn Fn()>,
}

/// `wire_chat_composer_streams` 的会话侧切片（初始化、活动会话、语言、草稿、角色）。
#[derive(Clone, Copy)]
pub struct WireComposerStreamsSessionSlice {
    pub initialized: RwSignal<bool>,
    pub chat: ChatSessionSignals,
    pub locale: RwSignal<Locale>,
    pub draft: RwSignal<String>,
    pub selected_agent_role: RwSignal<Option<String>>,
}

/// 流式发送路径的壳层与 UI 派生切片（与 SSE 回调、滚底、待发图共享）。
#[derive(Clone)]
pub struct WireComposerStreamsStreamSlice {
    /// 与 SSE 流式回调共享的壳层状态（见 [`ComposerStreamShell`]）。
    pub stream_shell: ComposerStreamShell,
    /// 见 [`ChatStreamBusyMemos::stream_turn_busy_ui`]。
    pub stream_turn_busy_ui: Memo<bool>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub pending_images: RwSignal<Vec<String>>,
}

/// `wire_chat_composer_streams` 的输入：按会话 / 流式两簇拆分，避免单结构体字段无序膨胀。
#[derive(Clone)]
pub struct WireComposerStreamsArgs {
    pub session: WireComposerStreamsSessionSlice,
    pub stream: WireComposerStreamsStreamSlice,
}
