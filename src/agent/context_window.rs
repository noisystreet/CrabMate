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
    ChatRequest, Message, Tool, is_chat_timeline_marker,
    is_message_excluded_from_llm_context_except_memory, message_content_as_str,
    message_content_into_text_lossy,
};
use crate::cm_agent::context_budget_pressure::{
    effective_summary_trigger_for_turn, resolve_context_budget_pressure,
    scale_message_pipeline_char_budget,
};
use log::{info, warn};
use reqwest::Client;

/// 用已解析的 user 模板填充占位符。
///
/// 支持 **`{max_tokens}`**（首选）与 **`{max_chars}`**（别名，同填 `context_summary_max_tokens`），
/// 以及 **`{transcript}`**。若模板缺少 `{transcript}`，告警并在末尾追加对话记录，避免空摘要请求。
pub(crate) fn format_context_summary_user(
    template: &str,
    max_tokens: u32,
    transcript: &str,
) -> String {
    let limit = max_tokens.to_string();
    let mut out = template
        .replace("{max_tokens}", &limit)
        .replace("{max_chars}", &limit);
    if out.contains("{transcript}") {
        out = out.replace("{transcript}", transcript);
    } else {
        warn!(
            target: "crabmate",
            "context_summary_user 模板缺少 {{transcript}} 占位符，已在末尾追加对话记录"
        );
        out.push_str("\n\n对话记录：\n\n");
        out.push_str(transcript);
    }
    out
}

/// 组装侧向摘要调用的 system + user（便于单测，不发起 LLM）。
pub(crate) fn build_context_summary_side_messages(
    cfg: &AgentConfig,
    transcript: &str,
) -> Vec<Message> {
    let system = {
        let s = cfg.context_pipeline.context_summary_system.trim();
        if s.is_empty() {
            crate::cm_config::embedded_context_summary_system().to_string()
        } else {
            s.to_string()
        }
    };
    let template = {
        let t = cfg.context_pipeline.context_summary_user_template.trim();
        if t.is_empty() {
            crate::cm_config::embedded_context_summary_user_template()
        } else {
            t
        }
    };
    let user = format_context_summary_user(
        template,
        cfg.context_pipeline.context_summary_max_tokens,
        transcript,
    );
    vec![Message::system_only(system), Message::user_only(user)]
}

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

fn complete_turn_group_tail_len(messages: &[Message], requested_tail: usize) -> Option<usize> {
    let groups = crate::agent::message_pipeline::conversation_turn_groups(messages);
    if groups.len() < 3 {
        return None;
    }
    let mut first_kept_group = groups.len().saturating_sub(2);
    while first_kept_group > 1
        && messages
            .len()
            .saturating_sub(groups[first_kept_group].start)
            < requested_tail
    {
        first_kept_group -= 1;
    }
    Some(
        messages
            .len()
            .saturating_sub(groups[first_kept_group].start),
    )
}

/// 每次调用模型前执行：经 [`apply_session_sync_pipeline`]（顺序见 `message_pipeline` 模块文档）。
///
/// - **Debug**（`RUST_LOG` 含 **`crabmate=debug`** 或 **`debug`**）：汇总一行 `message_pipeline session_sync: …`。
/// - **Trace**（**`crabmate::message_pipeline=trace`**）：每步一行 `session_sync_step stage=…`（可不开启全局 debug）。
pub fn prepare_messages_before_model_call_sync(
    messages: &mut Vec<Message>,
    cfg: &AgentConfig,
) -> crate::agent::message_pipeline::MessagePipelineDelta {
    prepare_messages_before_model_call_sync_with_budget(messages, cfg, None)
}

fn message_pipeline_config_for_turn(
    cfg: &AgentConfig,
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) -> crate::agent::message_pipeline::MessagePipelineConfig {
    let mut pipe = crate::agent::message_pipeline::MessagePipelineConfig::from(cfg);
    if cfg.llm_sampling.llm_context_tokens > 0 {
        // 已配置模型窗口时由最终请求 Token 预算器主导；字符预算仅保留为未配置窗口的降级路径。
        pipe.context_char_budget = 0;
    }
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
) -> crate::agent::message_pipeline::MessagePipelineDelta {
    let pipe_cfg = message_pipeline_config_for_turn(cfg, turn_budget);
    let need_report = log::log_enabled!(log::Level::Debug)
        || log::log_enabled!(target: "crabmate::message_pipeline", log::Level::Trace);
    let mut report = crate::agent::message_pipeline::MessagePipelineReport::default();
    let report_arg = need_report.then_some(&mut report);
    let delta = crate::agent::message_pipeline::apply_session_sync_pipeline_with_config(
        messages, pipe_cfg, report_arg,
    );
    if need_report {
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
    }
    delta
}

/// 同步裁剪 + 可选 LLM 摘要后的轻量结果（供时间线；不含 SSE）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrepareMessagesDelta {
    pub pipeline: crate::agent::message_pipeline::MessagePipelineDelta,
    pub summarized: bool,
    pub summary_tail_kept: Option<usize>,
    /// LLM 摘要替换「中间」时被移出的连续前缀区间（以摘要输入的会话坐标为基准）。
    pub summary_removed_prefix: crate::agent::message_pipeline::RemovedPrefix,
    pub compaction: crate::agent::context_compaction::ContextCompactionReport,
}

#[derive(Clone, Copy)]
struct PrepareSessionMessagesParams<'a> {
    tools: &'a [Tool],
    model_override: Option<&'a str>,
    cancel: Option<&'a std::sync::atomic::AtomicBool>,
    turn_budget: Option<&'a std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
}

#[derive(Clone, Copy)]
struct MaybeSummarizeParams<'a> {
    cancel: Option<&'a std::sync::atomic::AtomicBool>,
    turn_budget: Option<&'a std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
    force_for_token_budget: bool,
}

/// 同步裁剪 + 估算「发请求前」的 token 视图（不触发 LLM、不落盘）。
///
/// 供 [`prepare_session_messages_shared`] 与 [`prepare_readonly_model_messages`] 共用。
fn sync_trim_and_estimate_before(
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    tools: &[Tool],
    model_override: Option<&str>,
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) -> (
    crate::agent::message_pipeline::MessagePipelineDelta,
    crate::agent::context_compaction::ContextTokenEstimate,
) {
    let pipeline = prepare_messages_before_model_call_sync_with_budget(messages, cfg, turn_budget);
    let before_tokens = crate::agent::context_compaction::estimate_final_request_tokens(
        cfg,
        messages,
        tools,
        model_override,
    );
    (pipeline, before_tokens)
}

/// 应用 token 预算压缩，并在移除了完整回合组时清理孤儿工具消息。
fn finalize_token_budget_compaction(
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    tools: &[Tool],
    model_override: Option<&str>,
    before: crate::agent::context_compaction::ContextTokenEstimate,
    summarized_for_token_budget: bool,
) -> crate::agent::context_compaction::ContextCompactionReport {
    let compaction = crate::agent::context_compaction::compact_messages_to_token_budget(
        cfg,
        messages,
        tools,
        model_override,
        before,
        summarized_for_token_budget,
    );
    if compaction.removed_turn_groups > 0 {
        let _ = crate::agent::message_pipeline::drop_orphan_tool_messages(messages);
    }
    compaction
}

/// 主 Agent 外循环的「同步裁剪 + 可选 LLM 摘要」核心路径。
async fn prepare_session_messages_shared(
    llm_backend: &dyn ChatCompletionsBackend,
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    params: PrepareSessionMessagesParams<'_>,
) -> Result<PrepareMessagesDelta, Box<dyn std::error::Error + Send + Sync>> {
    let (pipeline, before_tokens) = sync_trim_and_estimate_before(
        cfg,
        messages,
        params.tools,
        params.model_override,
        params.turn_budget,
    );
    let force_summary_for_token_budget =
        crate::agent::context_compaction::ContextTokenBudget::from_config(cfg).is_some_and(
            |budget| before_tokens.used_input_tokens > budget.trigger_tokens,
        );
    let planned_summary_tail = complete_turn_group_tail_len(
        messages,
        cfg.context_pipeline
            .context_summary_tail_messages
            .clamp(4, 64),
    );
    let (summarized, summary_removed_prefix) = maybe_summarize_with_llm(
        llm_backend,
        client,
        api_key,
        cfg,
        messages,
        MaybeSummarizeParams {
            cancel: params.cancel,
            turn_budget: params.turn_budget,
            force_for_token_budget: force_summary_for_token_budget,
        },
    )
    .await?;
    let summary_tail_kept = summarized.then_some(planned_summary_tail.unwrap_or(0));
    let compaction = crate::agent::context_compaction::ContextCompactionReport {
        budget: crate::agent::context_compaction::ContextTokenBudget::from_config(cfg),
        before: before_tokens,
        after: crate::agent::context_compaction::estimate_final_request_tokens(
            cfg,
            messages,
            params.tools,
            params.model_override,
        ),
        removed_turn_groups: 0,
        removed_messages: 0,
        token_triggered: force_summary_for_token_budget,
        summarized_for_token_budget: summarized && force_summary_for_token_budget,
        provider_input_tokens: None,
        removed_prefix: crate::agent::message_pipeline::RemovedPrefix::default(),
    };
    Ok(PrepareMessagesDelta {
        pipeline,
        summarized,
        summary_tail_kept,
        summary_removed_prefix,
        compaction,
    })
}

fn context_summary_attempt_prep(
    cfg: &AgentConfig,
    messages: &[Message],
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
    force_for_token_budget: bool,
) -> Option<(usize, String)> {
    if !context_summary_trigger_reached(
        cfg,
        messages,
        turn_budget,
        force_for_token_budget,
    ) {
        return None;
    }
    if messages.first().is_none_or(|message| message.role != "system") {
        return None;
    }
    let requested_tail = cfg
        .context_pipeline
        .context_summary_tail_messages
        .clamp(4, 64);
    let tail = complete_turn_group_tail_len(messages, requested_tail)?;
    if messages.len() <= 1 + tail + 1 {
        return None;
    }
    let transcript = build_transcript_middle(
        messages,
        tail,
        cfg.context_pipeline.context_summary_transcript_max_chars,
    )?;
    Some((tail, transcript))
}

fn context_summary_trigger_reached(
    cfg: &AgentConfig,
    messages: &[Message],
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
    force_for_token_budget: bool,
) -> bool {
    if cfg.llm_sampling.llm_context_tokens > 0
        && cfg.context_pipeline.context_summary_trigger_chars == 0
        && !force_for_token_budget
    {
        return false;
    }
    let trigger = effective_summary_trigger_for_turn(cfg, turn_budget.map(|a| a.as_ref()));
    if trigger == 0 && !force_for_token_budget {
        return false;
    }
    force_for_token_budget
        || crate::agent::message_pipeline::estimate_non_system_chars(messages) >= trigger
}

fn build_context_summary_chat_request(cfg: &AgentConfig, transcript: &str) -> ChatRequest {
    let sum_messages = build_context_summary_side_messages(cfg, transcript);
    let llm_cfg = crate::cm_types::llm_config::LlmConfig {
        llm: cfg.llm.clone(),
        sampling: cfg.llm_sampling.clone(),
        vendor_flags: cfg.llm_vendor_flags.clone(),
        http_retry: cfg.llm_http_retry.clone(),
    };
    ChatRequest {
        core: crate::types::ChatRequestCore {
            model: cfg.llm.model.clone(),
            messages: sum_messages,
            tools: None,
            tool_choice: None,
            max_tokens: cfg.context_pipeline.context_summary_max_tokens,
            temperature: vendor_temperature_for_config(&llm_cfg, 0.2),
            seed: None,
            stream: None,
        },
        vendor: crate::llm::chat_request_vendor_extensions_for_agent(&llm_cfg),
    }
}

fn apply_llm_summary_to_messages(
    messages: &mut Vec<Message>,
    tail: usize,
    summary_text: &str,
) -> crate::agent::message_pipeline::RemovedPrefix {
    let tail_start = messages.len() - tail;
    let tail_part: Vec<Message> = messages[tail_start..].to_vec();
    messages.truncate(1);
    messages.push(Message::user_context_summary_injection(summary_text));
    messages.extend(tail_part);
    info!(
        target: "crabmate",
        "已用 LLM 压缩上下文 tail_kept={} new_len={}",
        tail,
        messages.len()
    );
    let _ = crate::agent::message_pipeline::drop_orphan_tool_messages(messages);
    // 保留首条 system，删掉其后的「中间」`[1, tail_start)`，再插入单条摘要 user。
    crate::agent::message_pipeline::RemovedPrefix {
        start: 1,
        removed: tail_start.saturating_sub(1),
    }
}

/// 当非 system 文本超过 `context_summary_trigger_chars` 时，调用模型生成摘要并替换「中间」为单条 user。
/// 返回 `(是否已替换, 被移出的连续前缀区间)`。
async fn maybe_summarize_with_llm(
    llm_backend: &dyn ChatCompletionsBackend,
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    params: MaybeSummarizeParams<'_>,
) -> Result<(bool, crate::agent::message_pipeline::RemovedPrefix), Box<dyn std::error::Error + Send + Sync>>
{
    let no_removal = crate::agent::message_pipeline::RemovedPrefix::default();
    let Some((tail, transcript)) = context_summary_attempt_prep(
        cfg,
        messages,
        params.turn_budget,
        params.force_for_token_budget,
    )
    else {
        return Ok((false, no_removal));
    };

    if let Some(budget) = params.turn_budget
        && let Err(deny) = budget.deny_llm_call_if_exhausted(&cfg.turn_budget)
    {
        warn!(
            target: "crabmate",
            "上下文摘要跳过：单轮预算已耗尽（{}）",
            deny.as_str()
        );
        return Ok((false, no_removal));
    }

    let req = build_context_summary_chat_request(cfg, &transcript);
    let cc = CompleteChatRetryingParams::new(
        llm_backend,
        client,
        api_key,
        cfg,
        LlmRetryingTransportOpts {
            cancel: params.cancel,
            ..LlmRetryingTransportOpts::headless_no_stream()
        },
        None,
        None,
    )
    .with_turn_budget(params.turn_budget);
    match complete_chat_retrying(&cc, &req).await {
        Ok((msg, _)) => {
            let summary_text = message_content_into_text_lossy(msg.content);
            if summary_text.trim().is_empty() {
                warn!(target: "crabmate", "上下文摘要模型返回空正文，跳过替换");
                return Ok((false, no_removal));
            }
            if summary_text.trim().chars().count() < 20 {
                warn!(
                    "context_window: LLM summary too short ({} chars), skipping replacement",
                    summary_text.trim().chars().count()
                );
                return Ok((false, no_removal));
            }
            let removed = apply_llm_summary_to_messages(messages, tail, &summary_text);
            Ok((true, removed))
        }
        Err(e) => {
            warn!(
                target: "crabmate",
                "上下文摘要请求失败，继续使用裁剪后的消息 error={}",
                e
            );
            Ok((false, no_removal))
        }
    }
}

/// 与 [`prepare_messages_for_model`] 搭配的**可选**回合侧挂钩：PER 层缓存失效 + `RunLoopTurnState` 缓冲代数。
pub struct PrepareMessagesForModelHooks<'a> {
    pub tools: &'a [Tool],
    pub model_override: Option<&'a str>,
    pub workspace_changelist:
        Option<&'a crate::workspace::changelist::WorkspaceChangelist>,
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
    hooks: PrepareMessagesForModelHooks<'_>,
) -> Result<PrepareMessagesDelta, Box<dyn std::error::Error + Send + Sync>> {
    let mut delta = prepare_session_messages_shared(
        llm_backend,
        client,
        api_key,
        cfg,
        messages,
        PrepareSessionMessagesParams {
            tools: hooks.tools,
            model_override: hooks.model_override,
            cancel: None,
            turn_budget: hooks.turn_budget,
        },
    )
    .await?;
    crate::workspace::changelist::sync_changelist_user_message(
        messages,
        hooks.workspace_changelist,
        cfg.session_workspace_changelist
            .session_workspace_changelist_enabled,
        cfg.session_workspace_changelist
            .session_workspace_changelist_max_chars,
    );
    delta.compaction = finalize_token_budget_compaction(
        cfg,
        messages,
        hooks.tools,
        hooks.model_override,
        delta.compaction.before,
        delta.compaction.summarized_for_token_budget,
    );
    if let Some(budget) = delta.compaction.budget {
        log::debug!(
            target: "crabmate::context_compaction",
            "context_compaction before_tokens={} after_tokens={} max_input_tokens={} trigger_tokens={} target_tokens={} reserved_output_tokens={} safety_margin_tokens={} message_tokens={} tool_schema_tokens={} attachment_tokens={} removed_turn_groups={} removed_messages={} counting_source={} reason={}",
            delta.compaction.before.used_input_tokens,
            delta.compaction.after.used_input_tokens,
            budget.max_input_tokens,
            budget.trigger_tokens,
            budget.target_tokens,
            budget.reserved_output_tokens,
            budget.safety_margin_tokens,
            delta.compaction.after.message_tokens,
            delta.compaction.after.tool_schema_tokens,
            delta.compaction.after.attachment_tokens,
            delta.compaction.removed_turn_groups,
            delta.compaction.removed_messages,
            delta
                .compaction
                .after
                .counting_source
                .map(crate::agent::context_compaction::ContextTokenCountingSource::as_str)
                .unwrap_or("unknown"),
            delta.compaction.compaction_reason(),
        );
    }
    if let Some(p) = hooks.per_coord_layer_cache {
        p.invalidate_workflow_validate_layer_cache_after_context_mutation();
    }
    if let Some(r) = hooks.run_loop_messages_revision {
        *r = r.wrapping_add(1);
    }
    delta.pipeline.n_after = messages.iter().filter(|m| !is_chat_timeline_marker(m)).count();
    Ok(delta)
}

/// 只读入口 [`prepare_readonly_model_messages`] 的可选 LLM 摘要依赖。
///
/// 仅在配置显式开启（`context_btw_summary_enabled`）时由调用方提供；`None` 即保持
/// 「只同步裁剪 + token 预算压缩、不额外调用模型」的旧行为。
pub struct ReadonlySummaryLlm<'a> {
    pub llm_backend: &'a dyn ChatCompletionsBackend,
    pub client: &'a Client,
    pub api_key: &'a str,
}

/// 只读、dry-run 的「发请求前」模型视图构建（旁路提问等不需要落盘的场景）。
///
/// 对传入的 `messages` 副本做**同步裁剪 + token 预算压缩**；当 `summary_llm` 为 `Some`
/// 时，还会按与主路径**相同**的触发规则尝试一次 LLM 摘要（仍**不**写 canonical / 归档 / artifact）。
/// 返回的 [`PrepareMessagesDelta`] 便于时间线与测试观察。
pub async fn prepare_readonly_model_messages(
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    tools: &[Tool],
    model_override: Option<&str>,
    summary_llm: Option<ReadonlySummaryLlm<'_>>,
) -> Result<PrepareMessagesDelta, Box<dyn std::error::Error + Send + Sync>> {
    let (pipeline, before_tokens) =
        sync_trim_and_estimate_before(cfg, messages, tools, model_override, None);
    let force_summary_for_token_budget =
        crate::agent::context_compaction::ContextTokenBudget::from_config(cfg).is_some_and(
            |budget| before_tokens.used_input_tokens > budget.trigger_tokens,
        );
    let (summarized, summary_removed_prefix, summary_tail_kept) = match summary_llm {
        Some(llm) => {
            let planned_summary_tail = complete_turn_group_tail_len(
                messages,
                cfg.context_pipeline.context_summary_tail_messages.clamp(4, 64),
            );
            let (summarized, removed) = maybe_summarize_with_llm(
                llm.llm_backend,
                llm.client,
                llm.api_key,
                cfg,
                messages,
                MaybeSummarizeParams {
                    cancel: None,
                    turn_budget: None,
                    force_for_token_budget: force_summary_for_token_budget,
                },
            )
            .await?;
            (
                summarized,
                removed,
                summarized.then_some(planned_summary_tail.unwrap_or(0)),
            )
        }
        None => (
            false,
            crate::agent::message_pipeline::RemovedPrefix::default(),
            None,
        ),
    };
    let compaction = finalize_token_budget_compaction(
        cfg,
        messages,
        tools,
        model_override,
        before_tokens,
        summarized && force_summary_for_token_budget,
    );
    Ok(PrepareMessagesDelta {
        pipeline,
        summarized,
        summary_tail_kept,
        summary_removed_prefix,
        compaction,
    })
}

#[cfg(test)]
mod tests;
