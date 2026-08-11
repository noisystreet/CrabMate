//! 宿主侧钩子：SSE 控制面、日志脱敏（与 `crabmate-internal` / `runtime` 解耦）。

use std::sync::atomic::AtomicBool;

use async_trait::async_trait;
use crabmate_types::{ChatRequest, Message};
use tokio::sync::mpsc::Sender;

use crate::call_error::LlmCallError;

/// 根 crate 实现的 `stream_chat` 侧效应（SSE、turn replay、redact 等）。
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
}
