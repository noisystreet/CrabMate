//! 单次 `chat/completions` 传输层入参（不含可变请求体）。

use std::sync::atomic::AtomicBool;

use reqwest::Client;
use tokio::sync::mpsc::Sender;

use crate::cm_types::llm_config::LlmHttpAuthMode;

use crate::cm_llm::stream_host::StreamChatHost;

/// **SSE / 流式 / 取消** 开关（各调用点差异主要在此）。
#[derive(Clone)]
pub struct LlmRetryingTransportOpts<'a> {
    pub out: Option<&'a Sender<String>>,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
}

impl<'a> LlmRetryingTransportOpts<'a> {
    /// 无 SSE、非流式、无取消（如上下文摘要等后台 `complete_chat_retrying`）。
    pub fn headless_no_stream() -> Self {
        Self {
            out: None,
            no_stream: true,
            cancel: None,
        }
    }
}

/// 单次 `chat/completions` 传输与展示开关（不含可变请求体）。
#[derive(Clone)]
pub struct StreamChatParams<'a> {
    pub host: &'a dyn StreamChatHost,
    pub client: &'a Client,
    pub api_key: &'a str,
    pub api_base: &'a str,
    pub auth_mode: LlmHttpAuthMode,
    pub out: Option<&'a Sender<String>>,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub fold_system_into_user: bool,
    /// Moonshot **kimi-k2.5** + 默认 thinking：含 **`tool_calls`** 的 assistant 须保留 **`reasoning_content`**。
    pub preserve_reasoning_on_assistant_tool_calls: bool,
    /// DeepSeek 思考模式：含 **`tool_calls`** 的 assistant 须在后续请求回传 **`reasoning_content`**。
    pub preserve_deepseek_thinking_reasoning_roundtrip: bool,
    /// 为 true 时经 SSE 下发结构化 **`thinking_trace`**（推理增量、终答阶段等），供 Web 调试台。
    pub thinking_trace_enabled: bool,
    /// 聊天附图目录（`POST /upload` 落盘）；视觉网关据此把 **`/uploads/`** 打成 **`data:`**。
    pub chat_uploads_dir: Option<&'a std::path::Path>,
    /// 当前工作区根；出站把用户消息里的 **`@` / `file:///`** 栅格图按 `read_file` 策略读盘。
    pub chat_workspace_root: Option<&'a std::path::Path>,
}
