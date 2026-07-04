//! 根包 [`crabmate_llm::StreamChatHost`] 实现（SSE、终端、redact、DSML）。

use std::io;
use std::sync::atomic::AtomicBool;

use async_trait::async_trait;
use crabmate_llm::{
    CliWaitSpinnerGuardHost, DsmlStreamFilter, LlmCallError, StreamChatHost,
    TerminalPlainFragmentCtx,
};
use crabmate_sse_protocol::StreamEndReason;
use crabmate_types::{ChatRequest, Message, message_content_as_str};
use log::{debug, error, info};
use tokio::sync::mpsc::Sender;

use crate::llm::terminal_render::{
    cli_terminal_write_plain_fragment, finalize_stream_plain_terminal_suffix,
    render_non_stream_assistant_terminal, terminal_render_agent_markdown,
};
use crate::redact::{
    self, CHAT_REQUEST_JSON_LOG_INFO_CHARS, CHAT_REQUEST_JSON_LOG_MAX_CHARS,
    HTTP_BODY_PREVIEW_LOG_CHARS,
};
use crate::runtime::cli_wait_spinner::CliWaitSpinnerGuard;
use crate::sse::{SsePayload, ThinkingTraceBody, encode_message};

const THINKING_TRACE_CHUNK_MAX: usize = 4096;

struct CrabmateDsmlStreamFilter(crate::dsml::StreamingDsmlContentFilter);

impl DsmlStreamFilter for CrabmateDsmlStreamFilter {
    fn push_chunk(&mut self, chunk: &str) -> String {
        self.0.push_chunk(chunk)
    }

    fn finish(&mut self) -> String {
        self.0.finish()
    }
}

struct CrabmateCliWaitSpinnerGuard(#[allow(dead_code)] CliWaitSpinnerGuard);

impl CliWaitSpinnerGuardHost for CrabmateCliWaitSpinnerGuard {}

fn clip_thinking_trace_text(s: &str) -> String {
    if s.len() <= THINKING_TRACE_CHUNK_MAX {
        return s.to_string();
    }
    let mut end = THINKING_TRACE_CHUNK_MAX;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::{THINKING_TRACE_CHUNK_MAX, clip_thinking_trace_text};

    #[test]
    fn thinking_trace_clip_respects_utf8_boundary() {
        let s = format!("{}你", "a".repeat(THINKING_TRACE_CHUNK_MAX - 1));
        let clipped = clip_thinking_trace_text(&s);

        assert!(clipped.ends_with('…'));
        assert!(clipped.is_char_boundary(clipped.len()));
    }
}

/// 进程内默认 [`StreamChatHost`]（Web / CLI / TUI 共用）。
#[derive(Debug, Copy, Clone, Default)]
pub struct CrabmateStreamChatHost;

#[async_trait]
impl StreamChatHost for CrabmateStreamChatHost {
    fn log_chat_request_json_preview_if_enabled(&self, req: &ChatRequest) {
        let as_debug = log::log_enabled!(log::Level::Debug);
        match serde_json::to_string(req) {
            Ok(body) => {
                if as_debug {
                    let preview = redact::preview_chars(&body, CHAT_REQUEST_JSON_LOG_MAX_CHARS);
                    debug!(
                        target: "crabmate",
                        "chat 请求体 JSON len={} messages_count={} body_preview={}",
                        body.len(),
                        req.messages.len(),
                        preview
                    );
                } else {
                    let preview = redact::preview_chars(&body, CHAT_REQUEST_JSON_LOG_INFO_CHARS);
                    info!(
                        target: "crabmate",
                        "chat 请求体 JSON len={} messages_count={} body_preview={}",
                        body.len(),
                        req.messages.len(),
                        preview
                    );
                }
            }
            Err(e) => {
                if as_debug {
                    debug!(
                        target: "crabmate",
                        "chat 请求体 JSON 序列化失败 err={}",
                        e
                    );
                } else {
                    info!(
                        target: "crabmate",
                        "chat 请求体 JSON 序列化失败 err={}",
                        e
                    );
                }
            }
        }
    }

    fn assistant_message_preview_for_log(&self, msg: &Message) -> String {
        redact::assistant_message_preview_for_log(msg)
    }

    fn append_stream_diagnostic_event(&self, stream_ended: &str, msg: &Message) {
        let answer_text = message_content_as_str(&msg.content).unwrap_or("");
        crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
            "stream_diagnostic",
            "final_stream_status",
            Some(&serde_json::json!({
                "phase": "stream_diagnostic",
                "stream_ended": stream_ended,
                "answer_phase": !answer_text.is_empty(),
                "delta_chars": answer_text.chars().count(),
            })),
        );
    }

    fn llm_call_error_from_http_api(&self, status_code: u16, body: &str) -> LlmCallError {
        let preview = redact::single_line_preview(body, HTTP_BODY_PREVIEW_LOG_CHARS);
        error!(
            target: "crabmate",
            "chat completions API 返回非成功状态 status={} body_len={} body_preview={}",
            status_code,
            body.len(),
            preview
        );
        let err_text = match redact::chat_api_error_message_for_user(body) {
            Some(m) => format!("模型接口返回错误（HTTP {status_code}）：{m}"),
            None => {
                format!("模型接口返回错误（HTTP {status_code}），请检查 API 密钥与配额，或稍后重试")
            }
        };
        LlmCallError::from_http_api(status_code, err_text)
    }

    fn boxed_non_stream_chat_parse_error(
        &self,
        body: &str,
        parse_err: &serde_json::Error,
    ) -> Box<dyn std::error::Error + Send + Sync> {
        let preview = redact::single_line_preview(body, HTTP_BODY_PREVIEW_LOG_CHARS);
        error!(
            target: "crabmate",
            "非流式 chat 响应 JSON 解析失败 err={} body_len={} body_preview={}",
            parse_err,
            body.len(),
            preview
        );
        Box::<dyn std::error::Error + Send + Sync>::from(
            "模型返回内容无法解析为预期格式，请稍后重试",
        )
    }

    async fn sse_out_send(
        &self,
        tx: &Sender<String>,
        line: String,
        context: &'static str,
        coop_cancel: Option<&AtomicBool>,
    ) -> bool {
        match coop_cancel {
            Some(c) => {
                crate::sse::send_string_logged_cooperative_cancel(tx, line, context, Some(c)).await
            }
            None => crate::sse::send_string_logged(tx, line, context).await,
        }
    }

    fn encode_assistant_answer_phase_sse(&self) -> String {
        encode_message(SsePayload::AssistantAnswerPhase {
            assistant_answer_phase: true,
        })
    }

    fn encode_parsing_tool_calls_sse(&self) -> String {
        encode_message(SsePayload::ParsingToolCalls {
            parsing_tool_calls: true,
        })
    }

    fn encode_turn_segment_start_sse(&self, tool_call_id: &str) -> String {
        encode_message(SsePayload::TurnSegmentStart {
            start: crate::sse::TurnSegmentStartBody {
                segment_id: format!("seg-before-{tool_call_id}"),
                kind: "commentary".to_string(),
                before_tool_call_id: Some(tool_call_id.to_string()),
            },
        })
    }

    fn encode_turn_segment_end_sse(&self, segment_id: &str) -> String {
        encode_message(SsePayload::TurnSegmentEnd {
            end: crate::sse::TurnSegmentEndBody {
                segment_id: segment_id.to_string(),
            },
        })
    }

    fn encode_thinking_trace_reasoning_delta_sse(&self, chunk: &str) -> String {
        encode_message(SsePayload::ThinkingTrace {
            trace: ThinkingTraceBody {
                op: "reasoning_delta".into(),
                node_id: Some("stream_reasoning".into()),
                parent_id: None,
                title: None,
                chunk: Some(clip_thinking_trace_text(chunk)),
                context_snapshot: None,
            },
        })
    }

    fn encode_thinking_trace_answer_phase_sse(&self) -> String {
        encode_message(SsePayload::ThinkingTrace {
            trace: ThinkingTraceBody {
                op: "answer_phase".into(),
                node_id: Some("stream_answer".into()),
                parent_id: Some("stream_reasoning".into()),
                title: Some("assistant_answer_phase".into()),
                chunk: None,
                context_snapshot: None,
            },
        })
    }

    fn new_dsml_stream_filter(&self, enabled: bool) -> Box<dyn DsmlStreamFilter> {
        Box::new(CrabmateDsmlStreamFilter(
            crate::dsml::StreamingDsmlContentFilter::new(enabled),
        ))
    }

    fn try_start_cli_wait_spinner(
        &self,
        cli_terminal_plain: bool,
    ) -> Option<Box<dyn CliWaitSpinnerGuardHost>> {
        Some(Box::new(CrabmateCliWaitSpinnerGuard(
            CliWaitSpinnerGuard::try_start_for_cli_plain_stream(cli_terminal_plain),
        )))
    }

    fn assistant_streaming_plain_concat(&self, msg: &Message) -> String {
        crate::runtime::message_display::assistant_streaming_plain_concat(msg)
    }

    fn cli_terminal_write_plain_fragment(
        &self,
        fragment: &str,
        ctx: TerminalPlainFragmentCtx<'_>,
        is_reasoning: bool,
    ) -> io::Result<()> {
        cli_terminal_write_plain_fragment(
            fragment,
            ctx.prefix_emitted,
            is_reasoning,
            ctx.reasoning_style_active,
        )
    }

    fn render_non_stream_assistant_terminal(
        &self,
        msg: &Message,
        plain_terminal_stream: bool,
        out_is_none: bool,
    ) -> io::Result<()> {
        render_non_stream_assistant_terminal(msg, plain_terminal_stream, out_is_none)
    }

    fn finalize_stream_plain_terminal_suffix(
        &self,
        cli_plain_reasoning_style_active: bool,
        cli_plain_prefix_emitted: bool,
        content_acc: &str,
        reasoning_acc: &str,
    ) -> io::Result<()> {
        finalize_stream_plain_terminal_suffix(
            cli_plain_reasoning_style_active,
            cli_plain_prefix_emitted,
            content_acc,
            reasoning_acc,
        )
    }

    fn terminal_render_agent_markdown(&self, md: &str) -> io::Result<()> {
        terminal_render_agent_markdown(md)
    }

    fn assistant_raw_markdown_body_from_parts(
        &self,
        reasoning_acc: &str,
        content_acc: &str,
    ) -> String {
        crate::runtime::message_display::assistant_raw_markdown_body_from_parts(
            reasoning_acc,
            content_acc,
        )
    }

    fn print_stream_end_reason_terminal(&self, reason: StreamEndReason) {
        let _ = crate::runtime::terminal_cli_transcript::print_stream_end_reason_terminal(reason);
    }
}

/// 进程内默认 [`StreamChatHost`] 实例。
pub static CRABMATE_STREAM_CHAT_HOST: CrabmateStreamChatHost = CrabmateStreamChatHost;
