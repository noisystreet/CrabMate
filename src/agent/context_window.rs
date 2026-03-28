//! 发往模型的 **`messages` 上下文策略**：工具结果截断、按条数/近似字符裁剪、可选 LLM 摘要。
//!
//! 同步变换的**步骤实现与编排**见 [`super::message_pipeline`]（[`apply_session_sync_pipeline`]）；本文件保留 **async 摘要**与对外的 `prepare_messages_for_model` 入口。

use log::{info, warn};
use reqwest::Client;
use tokio::sync::mpsc::Sender;

use crate::agent::per_coord::PerCoordinator;
use crate::config::AgentConfig;
use crate::llm::{ChatCompletionsBackend, complete_chat_retrying};
use crate::types::{ChatRequest, Message, is_message_excluded_from_llm_context_except_memory};

const SUMMARY_SYSTEM: &str = "你只负责压缩对话历史。使用简洁中文要点列表，保留：用户目标、关键路径/命令、错误信息、未决问题。不要编造事实。";

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
        .filter(|m| !is_message_excluded_from_llm_context_except_memory(m))
        .map(format_message_for_transcript)
        .collect();
    if s.chars().count() > cap {
        let take = cap.saturating_sub(80);
        s = s.chars().take(take).collect::<String>();
        s.push_str("\n[... 摘要输入过长，此处已截断 ...]");
    }
    Some(s)
}

/// 每次调用模型前执行：经 [`apply_session_sync_pipeline`]（顺序见 `message_pipeline` 模块文档）。
///
/// - **Debug**（`RUST_LOG` 含 **`crabmate=debug`** 或 **`debug`**）：汇总一行 `message_pipeline session_sync: …`。
/// - **Trace**（**`crabmate::message_pipeline=trace`**）：每步一行 `session_sync_step stage=…`（可不开启全局 debug）。
pub fn prepare_messages_before_model_call_sync(messages: &mut Vec<Message>, cfg: &AgentConfig) {
    let need_report = log::log_enabled!(log::Level::Debug)
        || log::log_enabled!(target: "crabmate::message_pipeline", log::Level::Trace);
    if need_report {
        let mut report = crate::agent::message_pipeline::MessagePipelineReport::default();
        crate::agent::message_pipeline::apply_session_sync_pipeline(
            messages,
            cfg,
            Some(&mut report),
        );
        log::debug!(
            target: "crabmate",
            "message_pipeline session_sync: {}",
            report.format_for_log()
        );
    } else {
        crate::agent::message_pipeline::apply_session_sync_pipeline(messages, cfg, None);
    }
}

/// 当非 system 文本超过 `context_summary_trigger_chars` 时，调用模型生成摘要并替换「中间」为单条 user。
pub async fn maybe_summarize_with_llm(
    llm_backend: &dyn ChatCompletionsBackend,
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if cfg.context_summary_trigger_chars == 0 {
        return Ok(());
    }
    let tail = cfg.context_summary_tail_messages.clamp(4, 64);
    let chars = crate::agent::message_pipeline::estimate_non_system_chars(messages);
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
        seed: None,
        stream: None,
    };

    match complete_chat_retrying(
        llm_backend,
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
            let _ = crate::agent::message_pipeline::drop_orphan_tool_messages(messages);
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
    llm_backend: &dyn ChatCompletionsBackend,
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    per_coord_layer_cache: Option<&mut PerCoordinator>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    prepare_messages_before_model_call_sync(messages, cfg);
    maybe_summarize_with_llm(llm_backend, client, api_key, cfg, messages).await?;
    if let Some(p) = per_coord_layer_cache {
        p.invalidate_workflow_validate_layer_cache_after_context_mutation();
    }
    Ok(())
}
