//! 发往模型的 **`messages` 上下文策略**：工具结果截断、按条数/近似字符裁剪、可选 LLM 摘要。
//!
//! 同步变换的**步骤实现与编排**见 [`super::message_pipeline`]（[`apply_session_sync_pipeline`]）；本文件保留 **async 摘要**与对外的 `prepare_messages_for_model` 入口。

use crate::agent::per_coord::PerCoordinator;
use crate::config::AgentConfig;
use crate::llm::{
    ChatCompletionsBackend, CompleteChatRetryingParams, LlmRetryingTransportOpts,
    complete_chat_retrying, vendor_temperature_for_config,
};
use crate::types::{
    ChatRequest, Message, is_message_excluded_from_llm_context_except_memory,
    message_content_as_str, message_content_into_text_lossy,
};
use crabmate_agent::context_budget_pressure::{
    effective_summary_trigger_for_turn, resolve_context_budget_pressure,
    scale_message_pipeline_char_budget,
};
use log::{info, warn};
use reqwest::Client;

const SUMMARY_SYSTEM: &str = "你只负责压缩对话历史。使用简洁中文要点列表，保留：用户目标、关键路径/命令、错误信息、未决问题。不要编造事实。";

fn format_message_for_transcript(m: &Message) -> String {
    let role = m.role.as_str();
    let body = if m.role == "assistant"
        && m.reasoning_content
            .as_deref()
            .is_some_and(|r| !r.trim().is_empty())
    {
        let r = m.reasoning_content.as_deref().unwrap_or("").trim();
        match message_content_as_str(&m.content)
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            Some(c) => format!("[reasoning]\n{r}\n\n[answer]\n{c}"),
            None => format!("[reasoning]\n{r}"),
        }
    } else if let Some(c) = crate::types::message_content_as_str(&m.content) {
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
    prepare_messages_before_model_call_sync_with_budget(messages, cfg, None);
}

fn message_pipeline_config_for_turn(
    cfg: &AgentConfig,
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) -> crate::agent::message_pipeline::MessagePipelineConfig {
    let mut pipe = crate::agent::message_pipeline::MessagePipelineConfig::from(cfg);
    let pressure = resolve_context_budget_pressure(cfg, turn_budget.map(|a| a.as_ref()));
    if pressure.char_budget_scale_percent < 100 {
        pipe.context_char_budget = scale_message_pipeline_char_budget(
            pipe.context_char_budget,
            pressure.char_budget_scale_percent,
        );
    }
    pipe
}

/// 与 [`prepare_messages_before_model_call_sync`] 相同，但当 [`TurnBudgetCounter`] 用量 ≥70%/≥90% 时收紧 char 预算裁剪。
pub fn prepare_messages_before_model_call_sync_with_budget(
    messages: &mut Vec<Message>,
    cfg: &AgentConfig,
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) {
    let pipe_cfg = message_pipeline_config_for_turn(cfg, turn_budget);
    let need_report = log::log_enabled!(log::Level::Debug)
        || log::log_enabled!(target: "crabmate::message_pipeline", log::Level::Trace);
    if need_report {
        let mut report = crate::agent::message_pipeline::MessagePipelineReport::default();
        crate::agent::message_pipeline::apply_session_sync_pipeline_with_config(
            messages,
            pipe_cfg,
            Some(&mut report),
        );
        let pressure = resolve_context_budget_pressure(cfg, turn_budget.map(|a| a.as_ref()));
        let tiktoken_note =
            crate::agent::tiktoken_prompt_tokens::prompt_token_count_vendor_shaped_for_session(
                cfg, messages,
            )
            .map(|t| {
                format!(
                    " | tiktoken_prompt_tokens≈{} (tiktoken_model={})",
                    t.prompt_tokens, t.tiktoken_model
                )
            })
            .unwrap_or_default();
        log::debug!(
            target: "crabmate",
            "message_pipeline session_sync: {}{} budget_pressure_char_scale={}",
            report.format_for_log(),
            tiktoken_note,
            pressure.char_budget_scale_percent
        );
    } else {
        crate::agent::message_pipeline::apply_session_sync_pipeline_with_config(
            messages, pipe_cfg, None,
        );
    }
}

/// 分层 **Manager** / 动态分解 / JSON 修复补调用等在组装 `no_tools_chat_request_for_hierarchical_manager` 之前，
/// 对**临时** `messages` 缓冲区跑与同进程主路径一致的会话同步管道（[`apply_session_sync_pipeline`]）。
///
/// 与 [`prepare_messages_before_model_call_sync`] 行为相同；单独命名便于检索「分层是否已走 message_pipeline」，
/// 并强调此处**不含** [`maybe_summarize_with_llm`]、工作区 changelist 注入、PER 缓存失效（与子目标隔离上下文一致）。
#[inline]
pub fn prepare_messages_for_hierarchical_llm_sync(messages: &mut Vec<Message>, cfg: &AgentConfig) {
    prepare_messages_before_model_call_sync(messages, cfg);
}

/// 分层 **Operator** ReAct：与主路径 [`prepare_messages_for_model`] 共用同步裁剪 + 可选 LLM 摘要；
/// **不含** changelist 注入与 PER 层缓存失效。
pub async fn prepare_messages_for_hierarchical_operator(
    llm_backend: &dyn ChatCompletionsBackend,
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    prepare_session_messages_shared(
        llm_backend,
        client,
        api_key,
        cfg,
        messages,
        cancel,
        turn_budget,
    )
    .await
}

/// 主 Agent 外循环与分层 Operator 共用的「同步裁剪 + 可选 LLM 摘要」核心路径。
pub(crate) async fn prepare_session_messages_shared(
    llm_backend: &dyn ChatCompletionsBackend,
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    prepare_messages_before_model_call_sync_with_budget(messages, cfg, turn_budget);
    maybe_summarize_with_llm(
        llm_backend,
        client,
        api_key,
        cfg,
        messages,
        cancel,
        turn_budget,
    )
    .await
}

/// 当非 system 文本超过 `context_summary_trigger_chars` 时，调用模型生成摘要并替换「中间」为单条 user。
pub async fn maybe_summarize_with_llm(
    llm_backend: &dyn ChatCompletionsBackend,
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let trigger = effective_summary_trigger_for_turn(cfg, turn_budget.map(|a| a.as_ref()));
    if trigger == 0 {
        return Ok(());
    }
    let tail = cfg
        .context_pipeline
        .context_summary_tail_messages
        .clamp(4, 64);
    let chars = crate::agent::message_pipeline::estimate_non_system_chars(messages);
    if chars < trigger {
        return Ok(());
    }
    if messages.is_empty() || messages[0].role != "system" {
        return Ok(());
    }
    if messages.len() <= 1 + tail + 1 {
        return Ok(());
    }
    let Some(transcript) = build_transcript_middle(
        messages,
        tail,
        cfg.context_pipeline.context_summary_transcript_max_chars,
    ) else {
        return Ok(());
    };

    if let Some(budget) = turn_budget
        && budget.deny_llm_call_if_exhausted(&cfg.turn_budget).is_err()
    {
        warn!(
            target: "crabmate",
            "上下文摘要跳过：已达单轮 LLM 调用或墙钟上限"
        );
        return Ok(());
    }

    let sum_messages = vec![
        Message::system_only(SUMMARY_SYSTEM.to_string()),
        Message::user_only(format!(
            "请将下列对话压缩为要点（不超过约 {} 字）。保留技术细节与待办：\n\n{}",
            cfg.context_pipeline.context_summary_max_tokens, transcript
        )),
    ];
    let req = ChatRequest {
        core: crate::types::ChatRequestCore {
            model: cfg.llm.model.clone(),
            messages: sum_messages,
            tools: None,
            tool_choice: None,
            max_tokens: cfg.context_pipeline.context_summary_max_tokens,
            temperature: {
                let llm_cfg = crabmate_types::llm_config::LlmConfig {
                    llm: cfg.llm.clone(),
                    sampling: cfg.llm_sampling.clone(),
                    vendor_flags: cfg.llm_vendor_flags.clone(),
                    http_retry: cfg.llm_http_retry.clone(),
                };
                vendor_temperature_for_config(&llm_cfg, 0.2)
            },
            seed: None,
            stream: None,
        },
        vendor: {
            let llm_cfg = crabmate_types::llm_config::LlmConfig {
                llm: cfg.llm.clone(),
                sampling: cfg.llm_sampling.clone(),
                vendor_flags: cfg.llm_vendor_flags.clone(),
                http_retry: cfg.llm_http_retry.clone(),
            };
            crate::llm::chat_request_vendor_extensions_for_agent(&llm_cfg)
        },
    };

    let cc = CompleteChatRetryingParams::new(
        llm_backend,
        client,
        api_key,
        cfg,
        LlmRetryingTransportOpts {
            cancel,
            ..LlmRetryingTransportOpts::headless_no_stream()
        },
        None,
        None,
    )
    .with_turn_budget(turn_budget);
    match complete_chat_retrying(&cc, &req).await {
        Ok((msg, _)) => {
            let summary_text = message_content_into_text_lossy(msg.content);
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
            messages.push(Message::user_only(format!(
                "[较早对话已摘要，以下为压缩要点]\n{}",
                summary_text.trim()
            )));
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

/// 与 [`prepare_messages_for_model`] 搭配的**可选**回合侧挂钩：PER 层缓存失效 + `RunLoopTurnState` 缓冲代数。
pub struct PrepareMessagesForModelHooks<'a> {
    pub per_coord_layer_cache: Option<&'a mut PerCoordinator>,
    pub run_loop_messages_revision: Option<&'a mut u64>,
    pub turn_budget: Option<&'a std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
}

/// 同步策略 + 可选异步摘要（在摘要前后都会再跑一遍同步压缩）。
pub async fn prepare_messages_for_model(
    llm_backend: &dyn ChatCompletionsBackend,
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    workspace_changelist: Option<&crate::workspace::changelist::WorkspaceChangelist>,
    hooks: PrepareMessagesForModelHooks<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    prepare_session_messages_shared(
        llm_backend,
        client,
        api_key,
        cfg,
        messages,
        None,
        hooks.turn_budget,
    )
    .await?;
    crate::workspace::changelist::sync_changelist_user_message(
        messages,
        workspace_changelist,
        cfg.session_workspace_changelist
            .session_workspace_changelist_enabled,
        cfg.session_workspace_changelist
            .session_workspace_changelist_max_chars,
    );
    if let Some(p) = hooks.per_coord_layer_cache {
        p.invalidate_workflow_validate_layer_cache_after_context_mutation();
    }
    if let Some(r) = hooks.run_loop_messages_revision {
        *r = r.wrapping_add(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageContent;

    #[test]
    fn prepare_messages_for_hierarchical_llm_sync_matches_session_sync() {
        let mut cfg = crate::config::load_config(None).expect("embed default");
        cfg.session_ui.max_message_history = 6;
        cfg.tool_transcript.tool_message_max_chars = 1_000_000;
        cfg.context_pipeline.context_char_budget = 0;

        let mut a = vec![
            Message::system_only("sys".to_string()),
            Message::user_only("task".to_string()),
        ];
        for i in 0..20 {
            a.push(Message {
                role: "assistant".to_string(),
                content: Some(MessageContent::Text(format!("step {i}"))),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            });
        }
        let mut b = a.clone();
        prepare_messages_before_model_call_sync(&mut a, &cfg);
        prepare_messages_for_hierarchical_llm_sync(&mut b, &cfg);
        assert_eq!(a, b);
    }

    #[test]
    fn budget_pressure_tightens_sync_pipeline_char_budget() {
        let mut cfg = crate::config::load_config(None).expect("embed default");
        cfg.context_pipeline.context_char_budget = 20_000;
        cfg.session_ui.max_message_history = 100;
        cfg.tool_transcript.tool_message_max_chars = 1_000_000;
        cfg.turn_budget.max_turn_tokens = 100;

        let budget = crate::agent::turn_budget::TurnBudgetCounter::new_shared();
        budget.record_estimated_tokens(75);

        let mut loose = vec![Message::system_only("s")];
        let mut tight = loose.clone();
        for i in 0..30 {
            let m = Message::user_only(format!("u{i}: {}", "x".repeat(800)));
            loose.push(m.clone());
            tight.push(m);
        }
        prepare_messages_before_model_call_sync(&mut loose, &cfg);
        prepare_messages_before_model_call_sync_with_budget(&mut tight, &cfg, Some(&budget));
        assert!(
            tight.len() <= loose.len(),
            "budget pressure should trim at least as aggressively"
        );
    }
}
