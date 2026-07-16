//! AG-UI 标准事件枚举（CrabMate 使用的子集）。
//!
//! 遵循 [AG-UI 协议](https://docs.ag-ui.com/concepts/events) 标准事件类型。
//! CrabMate 特有的非标准事件通过 `Custom` 变体承载。

use serde::Serialize;

/// AG-UI 标准事件（仅覆盖 CrabMate 需要的子集；待 V2Encoder 接入生产路径）。
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum AgUiEvent {
    // ── 生命周期 ──
    RunStarted {
        #[serde(rename = "threadId")]
        thread_id: String,
        #[serde(rename = "runId")]
        run_id: String,
    },
    RunFinished {
        #[serde(rename = "threadId")]
        thread_id: String,
        #[serde(rename = "runId")]
        run_id: String,
    },
    RunError {
        #[serde(rename = "threadId")]
        thread_id: String,
        #[serde(rename = "runId")]
        run_id: String,
        error: AgUiErrorBody,
    },

    // ── 文本消息 ──
    TextMessageStart {
        #[serde(rename = "messageId")]
        message_id: String,
        role: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },
    TextMessageContent {
        #[serde(rename = "messageId")]
        message_id: String,
        delta: String,
    },
    TextMessageEnd {
        #[serde(rename = "messageId")]
        message_id: String,
    },

    // ── 工具调用 ──
    ToolCallStart {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        name: String,
        #[serde(rename = "parentMessageId")]
        parent_message_id: String,
    },
    ToolCallArgs {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        args: String,
    },
    ToolCallEnd {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
    },
    ToolCallResult {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    // ── Reasoning（思维链） ──
    ReasoningMessageStart {
        #[serde(rename = "messageId")]
        message_id: String,
    },
    ReasoningMessageContent {
        #[serde(rename = "messageId")]
        message_id: String,
        delta: String,
    },
    ReasoningMessageEnd {
        #[serde(rename = "messageId")]
        message_id: String,
    },

    // ── 状态同步 ──
    StateSnapshot {
        state: serde_json::Value,
    },
    StateDelta {
        state: serde_json::Value,
    },

    // ── CrabMate 扩展 ──
    Custom {
        #[serde(rename = "customType")]
        custom_type: String,
        data: serde_json::Value,
    },
}

/// AG-UI 错误体（待 V2Encoder 接入生产路径）。
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AgUiErrorBody {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}
