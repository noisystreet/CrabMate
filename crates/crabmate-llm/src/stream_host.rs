//! 宿主侧钩子：SSE 控制面、终端渲染、日志脱敏、DSML 流式过滤（与 `crabmate-internal` / `runtime` 解耦）。

use std::io;
use std::sync::atomic::AtomicBool;

use async_trait::async_trait;
use crabmate_sse_protocol::StreamEndReason;
use crabmate_types::{ChatRequest, Message};
use tokio::sync::mpsc::Sender;

use crate::call_error::LlmCallError;

/// 流式正文下发前剥离 DSML 标记（展示路径；原文仍由 SSE 累积器保留）。
pub trait DsmlStreamFilter: Send {
    fn push_chunk(&mut self, chunk: &str) -> String;
    fn finish(&mut self) -> String;
}

/// CLI 纯文本流式片段写入时的可变状态（`Agent:` 前缀与 reasoning/content 分色）。
pub struct TerminalPlainFragmentCtx<'a> {
    pub prefix_emitted: &'a mut bool,
    pub reasoning_style_active: &'a mut bool,
}

/// RAII：drop 时结束 CLI 等待 spinner（无操作占位用于 Web / 非 plain 路径）。
pub trait CliWaitSpinnerGuardHost: Send {}

/// 根 crate 实现的 `stream_chat` 侧效应（SSE、终端、turn replay、redact 等）。
#[async_trait]
pub trait StreamChatHost: Send + Sync {
    fn log_chat_request_json_preview_if_enabled(&self, req: &ChatRequest);

    fn assistant_message_preview_for_log(&self, msg: &Message) -> String;

    fn append_stream_diagnostic_event(&self, stream_ended: &str, msg: &Message);

    fn llm_call_error_from_http_api(&self, status_code: u16, body: &str) -> LlmCallError;

    fn boxed_non_stream_chat_parse_error(
        &self,
        body: &str,
        parse_err: &serde_json::Error,
    ) -> Box<dyn std::error::Error + Send + Sync>;

    async fn sse_out_send(
        &self,
        tx: &Sender<String>,
        line: String,
        context: &'static str,
        coop_cancel: Option<&AtomicBool>,
    ) -> bool;

    fn encode_assistant_answer_phase_sse(&self) -> String;

    fn encode_parsing_tool_calls_sse(&self) -> String;

    /// `turn_segment_start`：`seg-before-{tool_call_id}`，工具前旁注锚点。
    fn encode_turn_segment_start_sse(&self, tool_call_id: &str) -> String;

    /// `turn_segment_end`：关闭指定 `segment_id`。
    fn encode_turn_segment_end_sse(&self, segment_id: &str) -> String;

    fn encode_thinking_trace_reasoning_delta_sse(&self, chunk: &str) -> String;

    fn encode_thinking_trace_answer_phase_sse(&self) -> String;

    /// 将推理文本增量编码为 SSE 行。V2 下包装为 REASONING_MESSAGE_CONTENT 事件；V1 返回原始文本。
    fn encode_reasoning_content_sse(&self, chunk: &str) -> String;

    /// 将终答文本增量编码为 SSE 行。V2 下包装为 TEXT_MESSAGE_CONTENT 事件；V1 返回原始文本。
    fn encode_answer_content_sse(&self, chunk: &str) -> String;

    /// 返回 TEXT_MESSAGE_START 的 SSE 行（仅 V2）；V1 返回空字符串。
    fn encode_text_message_start_sse(&self) -> String;

    fn new_dsml_stream_filter(&self, enabled: bool) -> Box<dyn DsmlStreamFilter>;

    fn try_start_cli_wait_spinner(
        &self,
        cli_terminal_plain: bool,
    ) -> Option<Box<dyn CliWaitSpinnerGuardHost>>;

    fn assistant_streaming_plain_concat(&self, msg: &Message) -> String;

    fn cli_terminal_write_plain_fragment(
        &self,
        fragment: &str,
        ctx: TerminalPlainFragmentCtx<'_>,
        is_reasoning: bool,
    ) -> io::Result<()>;

    fn render_non_stream_assistant_terminal(
        &self,
        msg: &Message,
        plain_terminal_stream: bool,
        out_is_none: bool,
    ) -> io::Result<()>;

    fn finalize_stream_plain_terminal_suffix(
        &self,
        cli_plain_reasoning_style_active: bool,
        cli_plain_prefix_emitted: bool,
        content_acc: &str,
        reasoning_acc: &str,
    ) -> io::Result<()>;

    fn terminal_render_agent_markdown(&self, md: &str) -> io::Result<()>;

    fn assistant_raw_markdown_body_from_parts(
        &self,
        reasoning_acc: &str,
        content_acc: &str,
    ) -> String;

    fn print_stream_end_reason_terminal(&self, reason: StreamEndReason);
}
