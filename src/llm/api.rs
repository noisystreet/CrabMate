//! OpenAI 兼容 **`chat/completions`** 的单次 HTTP 调用：SSE/JSON 解析、终端 Markdown 与 LaTeX→Unicode。
//!
//! 带 **tools** 的 `ChatRequest` 构造、**退避重试**与 Agent 侧调用入口见同目录 [`super`]（`llm`）；本模块专注传输与响应拼装。

use crate::types::{
    ChatRequest, FunctionCall, Message, StreamChunk, ToolCall, USER_CANCELLED_FINISH_REASON,
};
use futures_util::StreamExt;
use log::{debug, error, info};
use markdown_to_ansi::{Options, render};
use reqwest::Client;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::Sender;

use crate::redact::{self, CHAT_REQUEST_JSON_LOG_MAX_CHARS, HTTP_BODY_PREVIEW_LOG_CHARS};
use crate::runtime::message_display::assistant_markdown_source_for_display;

/// 在未开启 `RUST_LOG=…debug` 时，仍可用 **`AGENT_LOG_CHAT_REQUEST_JSON=1`** 在 **info** 级别打印请求体预览（与 `--log` 默认 `info` 配套）。
fn should_log_chat_request_json_preview() -> bool {
    log::log_enabled!(log::Level::Debug)
        || std::env::var_os("AGENT_LOG_CHAT_REQUEST_JSON").is_some_and(|v| {
            let s = v.to_string_lossy();
            let s = s.trim();
            !s.is_empty() && s != "0" && !s.eq_ignore_ascii_case("false")
        })
}

/// 流式正文 delta 在发往 `out`（SSE 等）前合并，减少 `mpsc` 次与 `String` 小片 clone。
/// 前端仍按 UTF-8 拼接，语义与逐 token 发送一致。
const SSE_STREAM_DELTA_FLUSH_BYTES: usize = 256;

async fn flush_sse_delta_buffer(pending: &mut String, tx: Option<&Sender<String>>) {
    if let Some(t) = tx
        && !pending.is_empty()
    {
        let _ = t.send(std::mem::take(pending)).await;
    }
}

/// 尝试获取终端宽度；获取失败时返回 None
fn terminal_width() -> Option<usize> {
    crossterm::terminal::size()
        .ok()
        .map(|(cols, _rows)| cols as usize)
        .filter(|w| *w > 0)
}

/// 按终端显示宽度估算行数（宽字符如中文按 2 列计）；仅单测使用——流式结束不再依赖行数做光标回退。
#[cfg(test)]
fn count_display_lines(content: &str, term_width: usize) -> usize {
    use unicode_width::UnicodeWidthStr;
    let w = term_width.max(1);
    content
        .split('\n')
        .map(|line| {
            let cols = line.width().max(1);
            cols.div_ceil(w)
        })
        .sum()
}

/// CLI：加粗着色 `Agent: ` + 助手展示管线（剥标签、规划可读化、LaTeX）+ `markdown_to_ansi`。
fn terminal_render_agent_markdown(content_acc: &str) -> io::Result<()> {
    debug!(
        target: "crabmate::print",
        "CLI 终端渲染助手 Markdown content_len={} content_preview={}",
        content_acc.len(),
        redact::preview_chars(content_acc, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    let term_w = terminal_width().unwrap_or(80);
    let mut stdout = io::stdout();
    crate::runtime::terminal_labels::write_agent_message_prefix(&mut stdout)?;
    stdout.flush()?;
    let opts = Options {
        syntax_highlight: true,
        width: Some(term_w),
        code_bg: true,
    };
    let content = assistant_markdown_source_for_display(content_acc);
    let rendered = render(&content, &opts);
    write!(stdout, "{}", rendered)?;
    if !rendered.ends_with('\n') {
        writeln!(stdout)?;
    }
    stdout.flush()
}

/// 解析 SSE 中一行 `data:` 后的 JSON 负载，累积正文与 tool_calls，并经 `out` 下发流式增量。
/// `pending_sse_delta`：仅当 `out` 为 `Some` 时使用；与 `content_acc` 同步追加，达阈值或发送控制帧前再 `send`。
#[allow(clippy::too_many_arguments)]
async fn ingest_sse_data_payload(
    payload: &str,
    out: Option<&Sender<String>>,
    pending_sse_delta: &mut String,
    reasoning_acc: &mut String,
    content_acc: &mut String,
    finish_reason: &mut String,
    tool_calls_acc: &mut Vec<(String, String, String, String)>,
    parsing_tool_calls_notified: &mut bool,
) {
    if payload.is_empty() {
        return;
    }
    let Ok(chunk) = serde_json::from_slice::<StreamChunk>(payload.as_bytes()) else {
        return;
    };
    let Some(choice) = chunk.choices.and_then(|c| c.into_iter().next()) else {
        return;
    };
    if let Some(reason) = choice.finish_reason
        && !reason.is_empty()
    {
        *finish_reason = reason;
    }
    let delta = choice.delta;
    if let Some(ref s) = delta.reasoning_content
        && !s.is_empty()
    {
        reasoning_acc.push_str(s);
        if let Some(tx) = out {
            pending_sse_delta.push_str(s);
            if pending_sse_delta.len() >= SSE_STREAM_DELTA_FLUSH_BYTES {
                let _ = tx.send(std::mem::take(pending_sse_delta)).await;
            }
        }
    }
    if let Some(ref s) = delta.content
        && !s.is_empty()
    {
        content_acc.push_str(s);
        if let Some(tx) = out {
            pending_sse_delta.push_str(s);
            if pending_sse_delta.len() >= SSE_STREAM_DELTA_FLUSH_BYTES {
                let _ = tx.send(std::mem::take(pending_sse_delta)).await;
            }
        }
    }
    if let Some(tcs) = delta.tool_calls {
        if !*parsing_tool_calls_notified && !tcs.is_empty() {
            *parsing_tool_calls_notified = true;
            if let Some(tx) = out {
                flush_sse_delta_buffer(pending_sse_delta, Some(tx)).await;
                let _ = tx
                    .send(crate::sse::encode_message(
                        crate::sse::SsePayload::ParsingToolCalls {
                            parsing_tool_calls: true,
                        },
                    ))
                    .await;
            }
        }
        for tc in tcs {
            let idx = tc.index;
            while tool_calls_acc.len() <= idx {
                tool_calls_acc.push((
                    String::new(),
                    "function".to_string(),
                    String::new(),
                    String::new(),
                ));
            }
            let acc = &mut tool_calls_acc[idx];
            if let Some(id) = tc.id {
                acc.0 = id;
            }
            if let Some(t) = tc.typ {
                acc.1 = t;
            }
            if let Some(f) = tc.function {
                if let Some(n) = f.name {
                    acc.2 = n;
                }
                if let Some(a) = f.arguments {
                    acc.3.push_str(&a);
                }
            }
        }
    }
}

/// 请求 chat/completions：`no_stream == false` 时为 SSE 流式；`true` 时为单次 JSON（`stream: false`）。
/// `render_to_terminal` 为 true 时：流式**不在**收包过程中写 stdout（避免半段 Markdown）；整段到达后与 **`--no-stream`** 相同，经 `markdown_to_ansi` 做基本 Markdown 渲染。非流式时在完整 `message` 到达后同样渲染。
/// 若提供 `out`，流式为每个 content delta；非流式则在有正文时整段发送一次（供 SSE 等）。
///
/// **非流式响应**：按 OpenAI 兼容形 `ChatResponse`（`choices[0].message` + `finish_reason`）反序列化；
/// DeepSeek 等兼容实现可用；字段形态不同的网关需在调用侧适配或扩展解析。
#[allow(clippy::too_many_arguments)] // HTTP + 流式/终端/out/cancel 为固定组合，拆结构体收益有限
pub async fn stream_chat(
    client: &Client,
    api_key: &str,
    api_base: &str,
    req: &mut ChatRequest,
    out: Option<&Sender<String>>,
    render_to_terminal: bool,
    no_stream: bool,
    cancel: Option<&AtomicBool>,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!(
        "{}/{}",
        api_base.trim_end_matches('/'),
        crate::types::OPENAI_CHAT_COMPLETIONS_REL_PATH
    );
    info!(
        target: "crabmate",
        "发起 chat 请求 url={} model={} streaming={}",
        url,
        req.model,
        !no_stream
    );
    // 与 `tool_chat_request` 重复一次：防止将来绕过构造器直接改 `ChatRequest.messages`，并兜底漏网相邻 assistant。
    req.messages = crate::types::normalize_messages_for_openai_compatible_request(std::mem::take(
        &mut req.messages,
    ));
    if should_log_chat_request_json_preview() {
        let as_debug = log::log_enabled!(log::Level::Debug);
        match serde_json::to_string(&*req) {
            Ok(body) => {
                let preview = redact::preview_chars(&body, CHAT_REQUEST_JSON_LOG_MAX_CHARS);
                if as_debug {
                    debug!(
                        target: "crabmate",
                        "chat 请求体 JSON len={} messages_count={} body_preview={}",
                        body.len(),
                        req.messages.len(),
                        preview
                    );
                } else {
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
    req.stream = Some(!no_stream);
    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&req)
        .send()
        .await?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        let preview = redact::single_line_preview(&body, HTTP_BODY_PREVIEW_LOG_CHARS);
        error!(
            target: "crabmate",
            "chat completions API 返回非成功状态 status={} body_len={} body_preview={}",
            status,
            body.len(),
            preview
        );
        let code = status.as_u16();
        let err_text = match redact::chat_api_error_message_for_user(&body) {
            Some(m) => format!("模型接口返回错误（HTTP {code}）：{m}"),
            None => format!("模型接口返回错误（HTTP {code}），请检查 API 密钥与配额，或稍后重试"),
        };
        return Err(err_text.into());
    }

    if no_stream {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(crate::types::LLM_CANCELLED_ERROR.into());
        }
        let body = res.text().await?;
        let parsed: crate::types::ChatResponse =
            serde_json::from_str(&body).map_err(|parse_err| {
                let preview = redact::single_line_preview(&body, HTTP_BODY_PREVIEW_LOG_CHARS);
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
            })?;
        let choice = parsed.choices.into_iter().next().ok_or_else(
            || -> Box<dyn std::error::Error + Send + Sync> {
                "非流式响应 choices 为空".into()
            },
        )?;
        let crate::types::Choice {
            message: msg,
            finish_reason,
        } = choice;

        let sse_plain = crate::runtime::message_display::assistant_streaming_plain_concat(&msg);
        if !sse_plain.is_empty()
            && let Some(tx) = out
        {
            let _ = tx.send(sse_plain).await;
        }
        if render_to_terminal {
            let md = crate::runtime::message_display::assistant_raw_markdown_body_for_message(&msg);
            if !md.is_empty() {
                terminal_render_agent_markdown(&md)?;
            }
        }
        if let Some(ref tcs) = msg.tool_calls
            && !tcs.is_empty()
            && let Some(tx) = out
        {
            let _ = tx
                .send(crate::sse::encode_message(
                    crate::sse::SsePayload::ParsingToolCalls {
                        parsing_tool_calls: true,
                    },
                ))
                .await;
        }
        debug!(
            target: "crabmate",
            "chat completions 非流式响应 finish_reason={} content_len={} tool_calls={} assistant_preview={}",
            finish_reason,
            msg.content.as_ref().map(|s| s.len()).unwrap_or(0),
            msg.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
            redact::assistant_message_preview_for_log(&msg)
        );
        return Ok((msg, finish_reason));
    }

    let mut stream = res.bytes_stream();
    let mut buf = Vec::new();
    let mut reasoning_acc = String::new();
    let mut content_acc = String::new();
    let mut pending_sse_delta = String::new();
    let mut tool_calls_acc: Vec<(String, String, String, String)> = Vec::new();
    let mut finish_reason = String::new();
    let mut parsing_tool_calls_notified = false;

    let mut stream_done = false;
    'stream_read: while let Some(chunk) = stream.next().await {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            break 'stream_read;
        }
        let chunk = chunk?;
        buf.extend_from_slice(&chunk);

        // 以“消费偏移”扫描完整行，避免每行 `split_off` 导致重复分配与拷贝。
        let mut consumed = 0usize;
        let mut cancelled = false;
        while let Some(rel_pos) = buf[consumed..].iter().position(|&b| b == b'\n') {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                cancelled = true;
                break;
            }
            let pos = consumed + rel_pos;
            let line = std::str::from_utf8(&buf[consumed..pos])
                .unwrap_or("")
                .trim();
            consumed = pos + 1;
            if let Some(payload) = line.strip_prefix("data: ").map(str::trim) {
                if payload == "[DONE]" {
                    stream_done = true;
                    break;
                }
                ingest_sse_data_payload(
                    payload,
                    out,
                    &mut pending_sse_delta,
                    &mut reasoning_acc,
                    &mut content_acc,
                    &mut finish_reason,
                    &mut tool_calls_acc,
                    &mut parsing_tool_calls_notified,
                )
                .await;
            }
        }
        if consumed > 0 {
            buf.drain(..consumed);
        }
        if cancelled || stream_done {
            break 'stream_read;
        }
    }
    // 连接关闭时，最后一条 `data:` 可能未带换行符，原先会一直留在 buf 中导致正文/工具参数尾部丢失。
    if !stream_done && !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.starts_with("data: ") {
            let payload = line.strip_prefix("data: ").unwrap_or("").trim();
            if payload != "[DONE]" {
                ingest_sse_data_payload(
                    payload,
                    out,
                    &mut pending_sse_delta,
                    &mut reasoning_acc,
                    &mut content_acc,
                    &mut finish_reason,
                    &mut tool_calls_acc,
                    &mut parsing_tool_calls_notified,
                )
                .await;
            }
        }
    }
    flush_sse_delta_buffer(&mut pending_sse_delta, out).await;
    // 流式阶段不向 stdout 逐 delta 打印（避免半段 Markdown）；整段结束后与 `--no-stream` 相同走 `terminal_render_agent_markdown`。
    // **不得**用 MoveUp + Clear 重绘：会与工具子进程 stdout 及真实折行错位。
    if render_to_terminal {
        let md = crate::runtime::message_display::assistant_raw_markdown_body_from_parts(
            reasoning_acc.as_str(),
            content_acc.as_str(),
        );
        if !md.is_empty() {
            terminal_render_agent_markdown(&md)?;
        }
    }
    let tool_calls = if tool_calls_acc.is_empty() {
        None
    } else {
        Some(
            tool_calls_acc
                .into_iter()
                .map(|(id, typ, name, arguments)| ToolCall {
                    id,
                    typ,
                    function: FunctionCall { name, arguments },
                })
                .collect(),
        )
    };
    let msg = Message {
        role: "assistant".to_string(),
        content: if content_acc.is_empty() {
            None
        } else {
            Some(content_acc)
        },
        reasoning_content: if reasoning_acc.is_empty() {
            None
        } else {
            Some(reasoning_acc)
        },
        tool_calls,
        name: None,
        tool_call_id: None,
    };
    let finish = if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        USER_CANCELLED_FINISH_REASON.to_string()
    } else {
        finish_reason
    };
    debug!(
        target: "crabmate",
        "chat completions 流式响应拼装完成 finish_reason={} content_len={} tool_calls={} assistant_preview={}",
        finish,
        msg.content.as_ref().map(|s| s.len()).unwrap_or(0),
        msg.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
        redact::assistant_message_preview_for_log(&msg)
    );
    Ok((msg, finish))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_display_lines() {
        assert_eq!(count_display_lines("a", 80), 1);
        assert_eq!(count_display_lines("a\nb", 80), 2);
        // 80 个 ASCII 占 80 列，一行刚好
        assert_eq!(count_display_lines(&"x".repeat(80), 80), 1);
        assert_eq!(count_display_lines(&"x".repeat(81), 80), 2);
        // 中文占 2 列，10 个中文 = 20 列
        assert_eq!(count_display_lines("中", 10), 1);
        assert_eq!(count_display_lines("中文中文中文", 10), 2); // 6 个中文 = 12 列，10 宽 => 2 行
    }
}
