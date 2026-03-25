//! 发往模型的 **`messages` 上下文策略**：工具结果截断、按条数/近似字符裁剪、可选 LLM 摘要。
//!
//! - **工具压缩**：缩小 `role: tool` 的 `content`，不改变消息条数（TUI 气泡仍在，仅变短）。
//! - **裁剪 / 摘要**：会 **删除** 或 **合并** 较早消息；TUI 在 `sync` 后聊天列表会相应变短，属预期权衡。

use log::{info, warn};
use reqwest::Client;
use tokio::sync::mpsc::Sender;

use crate::config::AgentConfig;
use crate::llm::complete_chat_retrying;
use crate::types::{ChatRequest, Message, is_chat_ui_separator};

const SUMMARY_SYSTEM: &str = "你只负责压缩对话历史。使用简洁中文要点列表，保留：用户目标、关键路径/命令、错误信息、未决问题。不要编造事实。";

/// 从字节长度近似字符数：ASCII 约 1:1，CJK 约 3:1，混合取中间值 ~2:1。
fn estimate_chars_from_bytes(s: &str) -> usize {
    s.len().div_ceil(2)
}

/// 估算单条消息占用的「约等于字符数」（用于预算；非精确 token）。
/// 使用字节长度近似，避免对大内容做 O(n) 的 `chars().count()`。
pub fn estimate_message_chars(m: &Message) -> usize {
    let mut n = m
        .content
        .as_deref()
        .map(estimate_chars_from_bytes)
        .unwrap_or(0);
    n = n.saturating_add(
        m.reasoning_content
            .as_deref()
            .map(estimate_chars_from_bytes)
            .unwrap_or(0),
    );
    if let Some(ref tcs) = m.tool_calls {
        for tc in tcs {
            n = n.saturating_add(tc.function.name.len());
            n = n.saturating_add(tc.function.arguments.len());
            n = n.saturating_add(tc.id.len());
        }
    }
    n
}

/// 除 `system` 外所有消息的近似字符总和。
pub fn estimate_non_system_chars(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| m.role != "system")
        .map(estimate_message_chars)
        .sum()
}

/// 截断 `tool` 消息正文（过长时追加说明尾注）。
pub fn compress_tool_message_contents(messages: &mut [Message], max_chars: usize) {
    let max_chars = max_chars.max(256);
    for m in messages.iter_mut() {
        if m.role != "tool" {
            continue;
        }
        let Some(ref c) = m.content else {
            continue;
        };
        // Fast path: if byte length is within limit, char count must also be
        if c.len() <= max_chars {
            continue;
        }
        let len = c.chars().count();
        if len <= max_chars {
            continue;
        }
        let truncated: String = c.chars().take(max_chars).collect();
        m.content = Some(format!(
            "{}\n\n[... 已截断，原始约 {} 字符，保留前 {} 字符 ...]",
            truncated, len, max_chars
        ));
    }
}

/// 保留首条 `system`，其后最多保留 `max_after_system` 条消息（与历史 `max_message_history` 语义一致）。
///
/// 与 `runtime/workspace_session` 加载截断一致：若保留的尾部以**两条连续** `assistant` 开头，且被裁掉的前缀里仍有 `user`，则插回其中最后一条 `user`（并丢掉一条较旧消息以维持条数上限），避免 `[system, assistant, assistant, …]` 触发 400。
pub fn trim_messages_by_count(messages: &mut Vec<Message>, max_after_system: usize) {
    if messages.is_empty() || max_after_system == 0 {
        return;
    }
    if messages[0].role == "system" {
        if messages.len() <= 1 + max_after_system {
            return;
        }
        let sys = messages[0].clone();
        let after: Vec<Message> = messages[1..].to_vec();
        let tail_keep = max_after_system;
        let skip = after.len().saturating_sub(tail_keep);
        let mut tail: Vec<Message> = after.iter().skip(skip).cloned().collect();
        let tail_opens_with_assistant_run = tail.len() >= 2
            && tail[0].role.trim().eq_ignore_ascii_case("assistant")
            && tail[1].role.trim().eq_ignore_ascii_case("assistant");
        if tail_opens_with_assistant_run
            && let Some(ui) = after[..skip]
                .iter()
                .rposition(|m| m.role.trim().eq_ignore_ascii_case("user"))
        {
            tail.insert(0, after[ui].clone());
            while tail.len() > tail_keep {
                if tail.len() <= 1 {
                    break;
                }
                tail.remove(1);
            }
        }
        let mut out = vec![sys];
        out.extend(tail);
        *messages = out;
    } else if messages.len() > max_after_system {
        let skip = messages.len() - max_after_system;
        *messages = messages.iter().skip(skip).cloned().collect();
    }
}

/// 在已压缩 tool 的前提下，从索引 1 起删除最旧消息，直到非 system 字符 ≤ `budget` 或条数触底。
pub fn trim_messages_by_char_budget(
    messages: &mut Vec<Message>,
    budget: usize,
    min_messages_after_system: usize,
) {
    if budget == 0 || messages.len() <= 1 {
        return;
    }
    let min_total = 1 + min_messages_after_system;
    if messages.len() <= min_total {
        return;
    }
    let current_chars = estimate_non_system_chars(messages);
    if current_chars <= budget {
        return;
    }

    let has_system_head = messages[0].role == "system";
    let start_idx = if has_system_head { 1 } else { 0 };
    let removable = messages.len().saturating_sub(min_total);
    if removable == 0 {
        return;
    }

    let mut remaining_chars = current_chars;
    let mut remove_count = 0usize;
    for msg in messages.iter().skip(start_idx).take(removable) {
        if remaining_chars <= budget {
            break;
        }
        remaining_chars = remaining_chars.saturating_sub(estimate_message_chars(msg));
        remove_count += 1;
    }
    if remove_count == 0 {
        return;
    }
    messages.drain(start_idx..start_idx + remove_count);
}

/// 删除「无前驱 `assistant` + `tool_calls`」的 `role: tool` 消息。
///
/// 按条数/字符裁剪历史时，可能截掉带 `tool_calls` 的 `assistant`，却保留其后的 `tool`，
/// OpenAI 兼容 API 会返回 400：`Messages with role 'tool' must be a response to a preceding message with 'tool_calls'`。
pub fn drop_orphan_tool_messages(messages: &mut Vec<Message>) {
    let mut keep = vec![true; messages.len()];
    for i in 0..messages.len() {
        if messages[i].role != "tool" {
            continue;
        }
        let has_valid_predecessor = i > 0 && {
            // Find the actual predecessor among kept messages
            let mut prev = i - 1;
            while prev > 0 && !keep[prev] {
                prev -= 1;
            }
            keep[prev]
                && (messages[prev].role == "tool"
                    || (messages[prev].role == "assistant"
                        && messages[prev]
                            .tool_calls
                            .as_ref()
                            .is_some_and(|c| !c.is_empty())))
        };
        if !has_valid_predecessor {
            keep[i] = false;
        }
    }
    let mut idx = 0;
    messages.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });
}

/// 每次调用模型前执行：工具压缩 → 条数裁剪 →（可选）字符预算裁剪。
pub fn prepare_messages_before_model_call_sync(messages: &mut Vec<Message>, cfg: &AgentConfig) {
    compress_tool_message_contents(messages, cfg.tool_message_max_chars);
    trim_messages_by_count(messages, cfg.max_message_history);
    if cfg.context_char_budget > 0 {
        trim_messages_by_char_budget(
            messages,
            cfg.context_char_budget,
            cfg.context_min_messages_after_system,
        );
        compress_tool_message_contents(messages, cfg.tool_message_max_chars);
    }
    drop_orphan_tool_messages(messages);
    crate::types::merge_consecutive_assistants_in_place(messages);
}

fn format_message_for_transcript(m: &Message) -> String {
    let role = m.role.as_str();
    let body = if m.role == "assistant"
        && m.reasoning_content
            .as_deref()
            .is_some_and(|r| !r.trim().is_empty())
    {
        let r = m.reasoning_content.as_deref().unwrap_or("").trim();
        match m
            .content
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            Some(c) => format!("[reasoning]\n{r}\n\n[answer]\n{c}"),
            None => format!("[reasoning]\n{r}"),
        }
    } else if let Some(c) = m.content.as_deref() {
        c.to_string()
    } else if let Some(ref tcs) = m.tool_calls {
        let args: Vec<String> = tcs
            .iter()
            .map(|tc| format!("{}({})", tc.function.name, tc.function.arguments))
            .collect();
        format!("[tool_calls] {}", args.join(", "))
    } else {
        String::new()
    };
    format!("{role}: {body}\n")
}

fn build_transcript_middle(messages: &[Message], tail: usize, cap: usize) -> Option<String> {
    if messages.len() <= 1 + tail + 1 {
        return None;
    }
    let end = messages.len() - tail;
    let mut s: String = messages[1..end]
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
        .map(format_message_for_transcript)
        .collect();
    if s.chars().count() > cap {
        let take = cap.saturating_sub(80);
        s = s.chars().take(take).collect::<String>();
        s.push_str("\n[... 摘要输入过长，此处已截断 ...]");
    }
    Some(s)
}

/// 当非 system 文本超过 `context_summary_trigger_chars` 时，调用模型生成摘要并替换「中间」为单条 user。
pub async fn maybe_summarize_with_llm(
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if cfg.context_summary_trigger_chars == 0 {
        return Ok(());
    }
    let tail = cfg.context_summary_tail_messages.clamp(4, 64);
    let chars = estimate_non_system_chars(messages);
    if chars < cfg.context_summary_trigger_chars {
        return Ok(());
    }
    if messages.is_empty() || messages[0].role != "system" {
        return Ok(());
    }
    if messages.len() <= 1 + tail + 1 {
        return Ok(());
    }
    let Some(transcript) =
        build_transcript_middle(messages, tail, cfg.context_summary_transcript_max_chars)
    else {
        return Ok(());
    };

    let sum_messages = vec![
        Message {
            role: "system".to_string(),
            content: Some(SUMMARY_SYSTEM.to_string()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "user".to_string(),
            content: Some(format!(
                "请将下列对话压缩为要点（不超过约 {} 字）。保留技术细节与待办：\n\n{}",
                cfg.context_summary_max_tokens, transcript
            )),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    ];
    let req = ChatRequest {
        model: cfg.model.clone(),
        messages: sum_messages,
        tools: None,
        tool_choice: None,
        max_tokens: cfg.context_summary_max_tokens,
        temperature: 0.2,
        stream: None,
    };

    match complete_chat_retrying(
        client,
        api_key,
        cfg,
        &req,
        None::<&Sender<String>>,
        false,
        true,
        None,
        false,
    )
    .await
    {
        Ok((msg, _)) => {
            let summary_text = msg.content.unwrap_or_default();
            if summary_text.trim().is_empty() {
                warn!(target: "crabmate", "上下文摘要模型返回空正文，跳过替换");
                return Ok(());
            }
            if summary_text.trim().chars().count() < 20 {
                warn!(
                    "context_window: LLM summary too short ({} chars), skipping replacement",
                    summary_text.trim().chars().count()
                );
                return Ok(());
            }
            let tail_start = messages.len() - tail;
            let tail_part: Vec<Message> = messages[tail_start..].to_vec();
            messages.truncate(1);
            messages.push(Message {
                role: "user".to_string(),
                content: Some(format!(
                    "[较早对话已摘要，以下为压缩要点]\n{}",
                    summary_text.trim()
                )),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            });
            messages.extend(tail_part);
            info!(
                target: "crabmate",
                "已用 LLM 压缩上下文 tail_kept={} new_len={}",
                tail,
                messages.len()
            );
            // 摘要替换中间消息后，仅需修复因删除带 tool_calls 的 assistant 而产生的孤立
            // tool 消息；compress/trim 在外层 prepare_messages_for_model 的首次同步中
            // 已完成，无需重跑。
            drop_orphan_tool_messages(messages);
        }
        Err(e) => {
            warn!(
                target: "crabmate",
                "上下文摘要请求失败，继续使用裁剪后的消息 error={}",
                e
            );
        }
    }
    Ok(())
}

/// 同步策略 + 可选异步摘要（在摘要前后都会再跑一遍同步压缩）。
pub async fn prepare_messages_for_model(
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    prepare_messages_before_model_call_sync(messages, cfg);
    maybe_summarize_with_llm(client, api_key, cfg, messages).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_msg(s: &str) -> Message {
        Message {
            role: "tool".to_string(),
            content: Some(s.to_string()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some("1".into()),
        }
    }

    #[test]
    fn compress_tool_truncates() {
        // 实现里对 max_chars 有下限 256，故用明显超长正文验证截断
        let long = "x".repeat(2000);
        let mut v = vec![tool_msg(&long)];
        compress_tool_message_contents(&mut v, 256);
        let c = v[0].content.as_deref().unwrap();
        assert!(c.starts_with(&"x".repeat(256)));
        assert!(c.contains("截断"));
        assert!(c.chars().count() < long.chars().count());
    }

    #[test]
    fn trim_by_count_keeps_system_and_tail() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("a".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("b".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("c".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        trim_messages_by_count(&mut v, 2);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].role, "system");
        assert_eq!(v[1].content.as_deref(), Some("b"));
        assert_eq!(v[2].content.as_deref(), Some("c"));
    }

    #[test]
    fn trim_by_count_inserts_user_when_tail_would_be_two_assistants() {
        let mut v = vec![
            Message::system_only("s"),
            Message::user_only("old_u"),
            Message {
                role: "assistant".to_string(),
                content: Some("a1".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("a2".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        trim_messages_by_count(&mut v, 2);
        assert_eq!(v.len(), 3);
        assert_eq!(v[1].role, "user");
        assert_eq!(v[1].content.as_deref(), Some("old_u"));
        assert_eq!(v[2].role, "assistant");
    }

    #[test]
    fn char_budget_drops_oldest_after_system() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("aaaaaaaaaa".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some("bbbbbbbbbbbbbbbb".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        trim_messages_by_char_budget(&mut v, 6, 1);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].content.as_deref(), Some("bbbbbbbbbbbbbbbb"));
    }

    fn assistant_with_tool_calls() -> Message {
        use crate::types::{FunctionCall, ToolCall};
        Message {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_string(),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: "x".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn drop_orphan_tool_removes_leading_tools_after_trim() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            tool_msg("orphan1"),
            tool_msg("orphan2"),
            Message {
                role: "user".to_string(),
                content: Some("last".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        drop_orphan_tool_messages(&mut v);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].role, "user");
        assert_eq!(v[1].content.as_deref(), Some("last"));
    }

    #[test]
    fn drop_orphan_tool_keeps_chain_after_assistant_tool_calls() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            assistant_with_tool_calls(),
            tool_msg("a"),
            tool_msg("b"),
            Message {
                role: "user".to_string(),
                content: Some("u".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        drop_orphan_tool_messages(&mut v);
        assert_eq!(v.len(), 5);
    }

    #[test]
    fn drop_orphan_tool_removes_tool_after_assistant_without_tool_calls() {
        let mut v = vec![
            Message {
                role: "system".to_string(),
                content: Some("s".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("text only".into()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            tool_msg("bad"),
        ];
        drop_orphan_tool_messages(&mut v);
        assert_eq!(v.len(), 2);
    }
}
