//! OpenAI 兼容 **`chat/completions`** 的单次 HTTP 调用：SSE/JSON 解析、终端 Markdown 与 LaTeX→Unicode。
//!
//! 带 **tools** 的 `ChatRequest` 构造、**退避重试**与 Agent 侧调用入口见同目录 [`super`]（`llm`）；本模块专注传输与响应拼装。

use crate::types::{
    ChatRequest, FunctionCall, Message, StreamChunk, ToolCall, USER_CANCELLED_FINISH_REASON,
};
use crossterm::{
    ExecutableCommand,
    cursor::{MoveToColumn, MoveUp},
    terminal::{Clear, ClearType},
};
use futures_util::StreamExt;
use markdown_to_ansi::{Options, render};
use reqwest::Client;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

use crate::redact::{self, HTTP_BODY_PREVIEW_LOG_CHARS};
use crate::runtime::latex_unicode::latex_math_to_unicode;

/// 尝试获取终端宽度；获取失败时返回 None
fn terminal_width() -> Option<usize> {
    crossterm::terminal::size()
        .ok()
        .map(|(cols, _rows)| cols as usize)
        .filter(|w| *w > 0)
}

/// 按终端显示宽度估算行数（宽字符如中文按 2 列计，避免换行错位）
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

/// 解析 SSE 中一行 `data:` 后的 JSON 负载，累积正文与 tool_calls，并经 `out` 下发流式增量。
#[allow(clippy::too_many_arguments)]
async fn ingest_sse_data_payload(
    payload: &str,
    out: Option<&Sender<String>>,
    render_to_terminal: bool,
    first_content: &mut bool,
    content_acc: &mut String,
    finish_reason: &mut String,
    tool_calls_acc: &mut Vec<(String, String, String, String)>,
    parsing_tool_calls_notified: &mut bool,
) {
    if payload.is_empty() {
        return;
    }
    let Ok(chunk) = serde_json::from_str::<StreamChunk>(payload) else {
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
    if let Some(ref s) = delta.content
        && !s.is_empty()
    {
        if let Some(tx) = out {
            let _ = tx.send(s.clone()).await;
        }
        if render_to_terminal {
            let mut stdout = io::stdout();
            if *first_content {
                let _ = write!(stdout, "Agent: ");
                let _ = stdout.flush();
                *first_content = false;
            }
            let _ = write!(stdout, "{}", s);
            let _ = stdout.flush();
        }
        content_acc.push_str(s);
    }
    if let Some(tcs) = delta.tool_calls {
        if !*parsing_tool_calls_notified && !tcs.is_empty() {
            *parsing_tool_calls_notified = true;
            if let Some(tx) = out {
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
/// `render_to_terminal` 仅在流式时边收边打；非流式时在完整 `message` 到达后一次性按 Markdown 渲染（若启用）。
/// 若提供 `out`，流式为每个 content delta；非流式则在有正文时整段发送一次（供 TUI/SSE 等）。
///
/// **非流式响应**：按 OpenAI 兼容形 `ChatResponse`（`choices[0].message` + `finish_reason`）反序列化；
/// DeepSeek 等兼容实现可用；字段形态不同的网关需在调用侧适配或扩展解析。
#[allow(clippy::too_many_arguments)] // HTTP + 流式/终端/out/cancel 为固定组合，拆结构体收益有限
pub async fn stream_chat(
    client: &Client,
    api_key: &str,
    api_base: &str,
    req: &ChatRequest,
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
        url = %url,
        model = %req.model,
        streaming = %(!no_stream),
        "发起 chat 请求"
    );
    let mut stream_req = req.clone();
    stream_req.stream = Some(!no_stream);
    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&stream_req)
        .send()
        .await?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        let preview = redact::single_line_preview(&body, HTTP_BODY_PREVIEW_LOG_CHARS);
        error!(
            status = %status,
            body_len = body.len(),
            body_preview = %preview,
            "chat completions API 返回非成功状态"
        );
        return Err(format!(
            "模型接口返回错误（HTTP {}），请检查 API 密钥与配额，或稍后重试",
            status.as_u16()
        )
        .into());
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
                    err = %parse_err,
                    body_len = body.len(),
                    body_preview = %preview,
                    "非流式 chat 响应 JSON 解析失败"
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

        if let Some(content) = msg.content.as_ref().filter(|c| !c.is_empty())
            && let Some(tx) = out
        {
            let _ = tx.send(content.clone()).await;
        }
        if render_to_terminal
            && let Some(ref content_acc) = msg.content
            && !content_acc.is_empty()
        {
            let term_w = terminal_width().unwrap_or(80);
            let mut stdout = io::stdout();
            write!(stdout, "Agent: ")?;
            stdout.flush()?;
            let opts = Options {
                syntax_highlight: true,
                width: Some(term_w),
                code_bg: true,
            };
            let content = latex_math_to_unicode(content_acc.trim());
            let rendered = render(&content, &opts);
            write!(stdout, "{}", rendered)?;
            if !rendered.ends_with('\n') {
                writeln!(stdout)?;
            }
            stdout.flush()?;
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
        return Ok((msg, finish_reason));
    }

    let mut stream = res.bytes_stream();
    let mut buf = Vec::new();
    let mut content_acc = String::new();
    let mut tool_calls_acc: Vec<(String, String, String, String)> = Vec::new();
    let mut finish_reason = String::new();
    let mut first_content = true;
    let mut parsing_tool_calls_notified = false;

    'stream_read: while let Some(chunk) = stream.next().await {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            break 'stream_read;
        }
        let chunk = chunk?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                break 'stream_read;
            }
            let line = std::str::from_utf8(&buf[..pos])
                .unwrap_or("")
                .trim()
                .to_string();
            buf = buf.split_off(pos + 1);
            if line.starts_with("data: ") {
                let payload = line.strip_prefix("data: ").unwrap_or("").trim();
                if payload == "[DONE]" {
                    break 'stream_read;
                }
                ingest_sse_data_payload(
                    payload,
                    out,
                    render_to_terminal,
                    &mut first_content,
                    &mut content_acc,
                    &mut finish_reason,
                    &mut tool_calls_acc,
                    &mut parsing_tool_calls_notified,
                )
                .await;
            }
        }
    }
    // 连接关闭时，最后一条 `data:` 可能未带换行符，原先会一直留在 buf 中导致正文/工具参数尾部丢失。
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.starts_with("data: ") {
            let payload = line.strip_prefix("data: ").unwrap_or("").trim();
            if payload != "[DONE]" {
                ingest_sse_data_payload(
                    payload,
                    out,
                    render_to_terminal,
                    &mut first_content,
                    &mut content_acc,
                    &mut finish_reason,
                    &mut tool_calls_acc,
                    &mut parsing_tool_calls_notified,
                )
                .await;
            }
        }
    }
    if render_to_terminal && !content_acc.is_empty() {
        let term_w = terminal_width().unwrap_or(80);
        let with_prefix = format!("Agent: {}", content_acc);
        let total_lines = count_display_lines(&with_prefix, term_w);
        // 光标上移、回到行首并清除至屏幕末尾，再重绘为 Markdown
        let mut stdout = io::stdout();
        stdout.execute(MoveUp(total_lines as u16))?;
        stdout.execute(MoveToColumn(0))?;
        stdout.execute(Clear(ClearType::FromCursorDown))?;
        write!(stdout, "Agent: ")?;
        stdout.flush()?;
        let opts = Options {
            syntax_highlight: true,
            width: Some(term_w),
            code_bg: true,
        };
        let content = latex_math_to_unicode(content_acc.trim());
        let rendered = render(&content, &opts);
        write!(stdout, "{}", rendered)?;
        if !rendered.ends_with('\n') {
            writeln!(stdout)?;
        }
        stdout.flush()?;
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
        tool_calls,
        name: None,
        tool_call_id: None,
    };
    let finish = if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        USER_CANCELLED_FINISH_REASON.to_string()
    } else {
        finish_reason
    };
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
