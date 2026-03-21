//! DeepSeek API 流式请求与 SSE 解析，终端 Markdown 渲染与数学公式（LaTeX→Unicode）

use crate::types::{ChatRequest, FunctionCall, Message, StreamChunk, ToolCall};
use crossterm::{
    cursor::{MoveToColumn, MoveUp},
    terminal::{Clear, ClearType},
    ExecutableCommand,
};
use futures_util::StreamExt;
use markdown_to_ansi::{render, Options};
use regex::Regex;
use reqwest::Client;
use std::io::{self, Write};
use tracing::{error, info};
use unicodeit::replace as latex_to_unicode;
use tokio::sync::mpsc::Sender;

/// 将文本中的 LaTeX 数学公式（$...$、$$...$$、\(...\)、\[...\]）转为 Unicode，便于终端显示
fn latex_math_to_unicode(s: &str) -> String {
    // 按顺序替换，避免 $ 与 \( 等嵌套问题
    let patterns = [
        (r"\\\[([\s\S]*?)\\\]", "display"), // \[ ... \]
        (r"\\\(([\s\S]*?)\\\)", "inline"),  // \( ... \)
        (r"\$\$([\s\S]*?)\$\$", "display"), // $$ ... $$
        (r"\$([^$\n]+)\$", "inline"),       // $ ... $（单行）
    ];
    let mut out = s.to_string();
    for (pat, _) in patterns {
        if let Ok(re) = Regex::new(pat) {
            out = re
                .replace_all(&out, |caps: &regex::Captures<'_>| {
                    let inner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                    latex_to_unicode(inner.trim())
                })
                .into_owned();
        }
    }
    out
}

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

/// 请求 chat/completions：`no_stream == false` 时为 SSE 流式；`true` 时为单次 JSON（`stream: false`）。
/// `render_to_terminal` 仅在流式时边收边打；非流式时在完整 `message` 到达后一次性按 Markdown 渲染（若启用）。
/// 若提供 `out`，流式为每个 content delta；非流式则在有正文时整段发送一次（供 TUI/SSE 等）。
///
/// **非流式响应**：按 OpenAI 兼容形 `ChatResponse`（`choices[0].message` + `finish_reason`）反序列化；
/// DeepSeek 等兼容实现可用；字段形态不同的网关需在调用侧适配或扩展解析。
pub async fn stream_chat(
    client: &Client,
    api_key: &str,
    api_base: &str,
    req: &ChatRequest,
    out: Option<&Sender<String>>,
    render_to_terminal: bool,
    no_stream: bool,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));
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
        error!(status = %status, body = %body, "API 返回错误");
        return Err(format!("API 错误 {}: {}", status, body).into());
    }

    if no_stream {
        let body = res.text().await?;
        let parsed: crate::types::ChatResponse = serde_json::from_str(&body).map_err(|e| {
            format!(
                "非流式响应 JSON 解析失败: {} — 正文开头: {}",
                e,
                body.chars().take(240).collect::<String>()
            )
        })?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
                "非流式响应 choices 为空".into()
            })?;
        let crate::types::Choice {
            message: msg,
            finish_reason,
        } = choice;

        if let Some(content) = msg.content.as_ref().filter(|c| !c.is_empty()) {
            if let Some(tx) = out {
                let _ = tx.send(content.clone()).await;
            }
        }
        if render_to_terminal {
            if let Some(ref content_acc) = msg.content {
                if !content_acc.is_empty() {
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
            }
        }
        return Ok((msg, finish_reason));
    }

    let mut stream = res.bytes_stream();
    let mut buf = Vec::new();
    let mut content_acc = String::new();
    let mut tool_calls_acc: Vec<(String, String, String, String)> = Vec::new();
    let mut finish_reason = String::new();
    let mut first_content = true;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line = std::str::from_utf8(&buf[..pos])
                .unwrap_or("")
                .trim()
                .to_string();
            buf = buf.split_off(pos + 1);
            if line.starts_with("data: ") {
                let payload = line.strip_prefix("data: ").unwrap_or("").trim();
                if payload == "[DONE]" {
                    break;
                }
                if payload.is_empty() {
                    continue;
                }
                let chunk: StreamChunk = match serde_json::from_str(payload) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let choice = match chunk.choices.and_then(|c| c.into_iter().next()) {
                    Some(c) => c,
                    None => continue,
                };
                if let Some(reason) = choice.finish_reason
                    && !reason.is_empty()
                {
                    finish_reason = reason;
                }
                let delta = choice.delta;
                if let Some(ref s) = delta.content
                    && !s.is_empty()
                {
                    if let Some(tx) = out {
                        let _ = tx.send(s.clone()).await;
                    }
                    if render_to_terminal {
                        // 这里保持“边收边打印”，但统一通过 stdout 句柄写入，便于后续进一步抽象终端输出。
                        let mut stdout = io::stdout();
                        if first_content {
                            write!(stdout, "Agent: ")?;
                            stdout.flush()?;
                            first_content = false;
                        }
                        write!(stdout, "{}", s)?;
                        stdout.flush()?;
                    }
                    content_acc.push_str(s);
                }
                if let Some(tcs) = delta.tool_calls {
                    for tc in tcs {
                        let idx = tc.index;
                        while tool_calls_acc.len() <= idx {
                            tool_calls_acc.push((String::new(), "function".to_string(), String::new(), String::new()));
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
        content: if content_acc.is_empty() { None } else { Some(content_acc) },
        tool_calls,
        name: None,
        tool_call_id: None,
    };
    Ok((msg, finish_reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latex_math_to_unicode_inline() {
        // $x$ -> 转为 Unicode 字母 x（unicodeit 可能保留或转换）
        let out = latex_math_to_unicode("公式 $x^2$ 结束");
        assert!(!out.contains("$"));
        assert!(out.contains("x") || out.contains("²"));
    }

    #[test]
    fn test_latex_math_to_unicode_display() {
        let out = latex_math_to_unicode("$$1+1=2$$");
        assert!(!out.contains("$$"));
    }

    #[test]
    fn test_latex_math_to_unicode_plain_unchanged() {
        let s = "纯文本无公式";
        assert_eq!(latex_math_to_unicode(s), s);
    }

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
