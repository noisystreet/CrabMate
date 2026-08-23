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
    ChatRequest, Message, is_chat_timeline_marker,
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
}

/// 主 Agent 外循环的「同步裁剪 + 可选 LLM 摘要」核心路径。
pub(crate) async fn prepare_session_messages_shared(
    llm_backend: &dyn ChatCompletionsBackend,
    client: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) -> Result<PrepareMessagesDelta, Box<dyn std::error::Error + Send + Sync>> {
    let pipeline = prepare_messages_before_model_call_sync_with_budget(messages, cfg, turn_budget);
    let summarized = maybe_summarize_with_llm(
        llm_backend,
        client,
        api_key,
        cfg,
        messages,
        cancel,
        turn_budget,
    )
    .await?;
    let summary_tail_kept = summarized.then_some(
        cfg.context_pipeline
            .context_summary_tail_messages
            .clamp(4, 64),
    );
    Ok(PrepareMessagesDelta {
        pipeline,
        summarized,
        summary_tail_kept,
    })
}

fn context_summary_attempt_prep(
    cfg: &AgentConfig,
    messages: &[Message],
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) -> Option<(usize, String)> {
    let trigger = effective_summary_trigger_for_turn(cfg, turn_budget.map(|a| a.as_ref()));
    if trigger == 0 {
        return None;
    }
    let tail = cfg
        .context_pipeline
        .context_summary_tail_messages
        .clamp(4, 64);
    let chars = crate::agent::message_pipeline::estimate_non_system_chars(messages);
    if chars < trigger {
        return None;
    }
    if messages.is_empty() || messages[0].role != "system" {
        return None;
    }
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

fn apply_llm_summary_to_messages(messages: &mut Vec<Message>, tail: usize, summary_text: &str) {
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
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let Some((tail, transcript)) = context_summary_attempt_prep(cfg, messages, turn_budget) else {
        return Ok(false);
    };

    if let Some(budget) = turn_budget
        && budget.deny_llm_call_if_exhausted(&cfg.turn_budget).is_err()
    {
        warn!(
            target: "crabmate",
            "上下文摘要跳过：已达单轮 LLM 调用或墙钟上限"
        );
        return Ok(false);
    }

    let req = build_context_summary_chat_request(cfg, &transcript);
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
                return Ok(false);
            }
            if summary_text.trim().chars().count() < 20 {
                warn!(
                    "context_window: LLM summary too short ({} chars), skipping replacement",
                    summary_text.trim().chars().count()
                );
                return Ok(false);
            }
            apply_llm_summary_to_messages(messages, tail, &summary_text);
            Ok(true)
        }
        Err(e) => {
            warn!(
                target: "crabmate",
                "上下文摘要请求失败，继续使用裁剪后的消息 error={}",
                e
            );
            Ok(false)
        }
    }
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
) -> Result<PrepareMessagesDelta, Box<dyn std::error::Error + Send + Sync>> {
    let mut delta = prepare_session_messages_shared(
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
    delta.pipeline.n_after = messages.iter().filter(|m| !is_chat_timeline_marker(m)).count();
    Ok(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验收辅助：摘要正文是否覆盖给定锚点（路径/错误串等）。
    fn context_summary_covers_anchors(summary: &str, anchors: &[&str]) -> bool {
        anchors
            .iter()
            .all(|a| !a.is_empty() && summary.contains(*a))
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

    #[test]
    fn format_context_summary_user_replaces_placeholders() {
        let out = format_context_summary_user(
            "上限 {max_tokens}\n---\n{transcript}\n---",
            512,
            "user: 修 src/foo.rs\nerror: E0308",
        );
        assert!(out.contains("512"));
        assert!(out.contains("src/foo.rs"));
        assert!(out.contains("E0308"));
        assert!(!out.contains("{max_tokens}"));
        assert!(!out.contains("{transcript}"));
    }

    #[test]
    fn format_context_summary_user_accepts_max_chars_alias() {
        let out = format_context_summary_user("n={max_chars}\n{transcript}", 256, "path.rs");
        assert!(out.contains("n=256"));
        assert!(out.contains("path.rs"));
    }

    #[test]
    fn format_context_summary_user_appends_when_transcript_placeholder_missing() {
        let out = format_context_summary_user("只有骨架无占位", 128, "crates/demo/src/path_bug.rs");
        assert!(out.contains("只有骨架无占位"));
        assert!(out.contains("crates/demo/src/path_bug.rs"));
        assert!(out.contains("对话记录："));
    }

    #[test]
    fn loaded_summary_prompts_require_retention_and_structure() {
        let cfg = crate::config::load_config(None).expect("embed default");
        let sys = &cfg.context_pipeline.context_summary_system;
        for needle in ["必须保留", "禁止编造", "关键路径", "错误信息", "未决"] {
            assert!(
                sys.contains(needle),
                "context_summary_system missing `{needle}`"
            );
        }
        let user_t = &cfg.context_pipeline.context_summary_user_template;
        for needle in [
            "## 目标",
            "## 已完成",
            "## 未决",
            "## 关键路径与错误",
            "{transcript}",
        ] {
            assert!(
                user_t.contains(needle),
                "context_summary_user_template missing `{needle}`"
            );
        }
        assert!(
            user_t.contains("{max_tokens}") || user_t.contains("{max_chars}"),
            "user template should mention a length placeholder"
        );
    }

    #[test]
    fn side_messages_embed_fixture_transcript_anchors() {
        let cfg = crate::config::load_config(None).expect("embed default");
        let transcript = concat!(
            "user: 修复 crates/demo/src/path_bug.rs 的类型错误\n",
            "assistant: [tool_calls] read_file({\"path\":\"crates/demo/src/path_bug.rs\"})\n",
            "tool: error[E0308]: mismatched types in path_bug.rs\n",
        );
        let msgs = build_context_summary_side_messages(&cfg, transcript);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        let user = crate::types::message_content_as_str(&msgs[1].content).unwrap_or("");
        assert!(
            context_summary_covers_anchors(
                user,
                &[
                    "crates/demo/src/path_bug.rs",
                    "error[E0308]",
                    "## 目标",
                    "## 关键路径与错误",
                ]
            ),
            "summary side user must carry transcript anchors and skeleton; got:\n{user}"
        );
    }

    #[test]
    fn fixture_good_summary_covers_path_and_error() {
        // 文档化验收：合格摘要须保留路径与错误锚点（供日后接 mock LLM 评测复用）。
        let good = concat!(
            "## 目标\n修复 path_bug 类型错误\n\n",
            "## 已完成\nread_file crates/demo/src/path_bug.rs\n\n",
            "## 未决\n尚未 patch\n\n",
            "## 关键路径与错误\ncrates/demo/src/path_bug.rs；error[E0308]: mismatched types\n",
        );
        assert!(context_summary_covers_anchors(
            good,
            &["crates/demo/src/path_bug.rs", "error[E0308]"]
        ));
        let bad = "## 目标\n修个文件\n\n## 关键路径与错误\n某个源码里类型不对\n";
        assert!(!context_summary_covers_anchors(
            bad,
            &["crates/demo/src/path_bug.rs", "error[E0308]"]
        ));
    }

    #[test]
    fn apply_llm_summary_writes_named_injection_and_keeps_tail() {
        let mut messages = vec![
            Message::system_only("sys"),
            Message::user_only("old-1"),
            Message::assistant_only("a1"),
            Message::user_only("tail-user"),
            Message::assistant_only("tail-a"),
        ];
        apply_llm_summary_to_messages(&mut messages, 2, "要点：修了 path_bug");
        assert_eq!(messages[0].role, "system");
        assert!(crate::types::is_context_summary_injection(&messages[1]));
        assert_eq!(
            messages[1].name.as_deref(),
            Some(crate::types::CRABMATE_CONTEXT_SUMMARY_NAME)
        );
        assert!(!crate::types::user_message_counts_for_branch_truncation(
            &messages[1]
        ));
        assert_eq!(messages.len(), 4);
        assert_eq!(
            crate::types::message_content_as_str(&messages[2].content),
            Some("tail-user")
        );
        let snap = crate::types::filter_messages_for_web_client_snapshot(&messages);
        assert!(
            snap.iter()
                .all(|m| !crate::types::is_context_summary_injection(m))
        );
        assert!(
            snap.iter()
                .any(|m| crate::types::message_content_as_str(&m.content) == Some("tail-user"))
        );
        let vendor = crate::types::messages_for_api_stripping_reasoning_skip_ui_separators(
            &messages, false, false,
        );
        assert!(
            vendor
                .iter()
                .any(crate::types::is_context_summary_injection),
            "summary must still go to the vendor"
        );
    }

    #[test]
    fn legacy_unnamed_summary_prefix_stays_persisted_and_hidden() {
        let legacy = Message::user_only(format!(
            "{}\n旧会话无 name",
            crate::types::CONTEXT_SUMMARY_INJECTION_CONTENT_PREFIX
        ));
        assert!(crate::types::is_context_summary_injection(&legacy));
        assert!(!crate::types::user_message_counts_for_branch_truncation(
            &legacy
        ));
        let mut v = vec![Message::user_only("真实"), legacy.clone()];
        crate::types::strip_orchestration_injected_users_for_conversation_store(&mut v);
        assert_eq!(v.len(), 2);
        assert_eq!(
            crate::types::filter_messages_for_web_client_snapshot(&v).len(),
            1
        );
    }
}
