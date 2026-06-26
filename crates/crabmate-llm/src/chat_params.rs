//! 单次 `chat/completions` 传输层入参（不含可变请求体）。

use std::sync::atomic::AtomicBool;

use reqwest::Client;
use tokio::sync::mpsc::Sender;

use crabmate_config::LlmHttpAuthMode;

use crate::stream_host::StreamChatHost;
use crate::stream_scratch::TuiLlmStreamScratchArc;

/// **SSE / 终端 / 流式 / 取消** 开关（各调用点差异主要在此）。
#[derive(Clone)]
pub struct LlmRetryingTransportOpts<'a> {
    pub out: Option<&'a Sender<String>>,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub plain_terminal_stream: bool,
    /// 全屏 TUI：`suppress_stdout_render` 时经 SSE 解析线程写入，UI 线程轮询展示。
    pub tui_llm_stream_scratch: Option<TuiLlmStreamScratchArc>,
}

impl<'a> LlmRetryingTransportOpts<'a> {
    /// 无 SSE、非流式、无取消（如上下文摘要等后台 `complete_chat_retrying`）。
    pub fn headless_no_stream() -> Self {
        Self {
            out: None,
            render_to_terminal: false,
            no_stream: true,
            cancel: None,
            plain_terminal_stream: false,
            tui_llm_stream_scratch: None,
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
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub plain_terminal_stream: bool,
    pub fold_system_into_user: bool,
    /// Moonshot **kimi-k2.5** + 默认 thinking：含 **`tool_calls`** 的 assistant 须保留 **`reasoning_content`**。
    pub preserve_reasoning_on_assistant_tool_calls: bool,
    /// DeepSeek 思考模式：含 **`tool_calls`** 的 assistant 须在后续请求回传 **`reasoning_content`**。
    pub preserve_deepseek_thinking_reasoning_roundtrip: bool,
    /// 为 true 时经 SSE 下发结构化 **`thinking_trace`**（推理增量、终答阶段等），供 Web 调试台。
    pub thinking_trace_enabled: bool,
    /// 流式正文下发前经 DSML 过滤器剥离（实现位于根包 `dsml`）。
    pub dsml_stream_strip_enabled: bool,
    pub tui_llm_stream_scratch: Option<TuiLlmStreamScratchArc>,
}
