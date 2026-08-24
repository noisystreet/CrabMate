//! 使用 **tiktoken-rs** 对「与 [`crate::agent::message_pipeline::conversation_messages_to_vendor_body`] 一致」的
//! `messages` 做 **OpenAI Chat Completions** 风格的 prompt token 近似计数。消息级 API 不含 `tools` JSON 与图片细项；
//! 最终请求分项由 `agent::context_compaction` 叠加计算。
//!
//! 未知 `model` id 时按 **`gpt-4` → `gpt-4o`** 顺序回落，以便 DeepSeek / Kimi 等 OpenAI 兼容网关仍能给出**可比**的粗估值（与真实网关分词可能仍有偏差，见 API 字段说明）。

use tiktoken_rs::{ChatCompletionRequestMessage, FunctionCall, bpe_for_model, num_tokens_from_messages};

use crate::agent::message_pipeline::conversation_messages_to_vendor_body;
use crate::config::AgentConfig;
use crate::llm::{
    fold_system_into_user_for_config, llm_vendor_adapter, vendor::deepseek_json_output_eligible,
};
use crate::types::{Message, MessageContent};

pub use crate::cm_types::TiktokenPromptTokensSnapshot;

fn ping_message() -> ChatCompletionRequestMessage {
    ChatCompletionRequestMessage {
        role: "user".to_string(),
        content: Some("ping".to_string()),
        ..Default::default()
    }
}

fn crabmate_message_to_tiktoken(m: &Message) -> ChatCompletionRequestMessage {
    let mut body = match &m.content {
        None => String::new(),
        Some(MessageContent::Text(text)) => text.clone(),
        Some(MessageContent::Parts(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
    };
    let extra_reasoning = if m.role == "assistant" {
        m.reasoning_content.as_deref().and_then(|s| {
            let t = s.trim();
            (!t.is_empty()).then_some(t)
        })
    } else {
        None
    };
    if let Some(r) = extra_reasoning {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(r);
    }
    let tool_calls: Vec<FunctionCall> = m
        .tool_calls
        .as_ref()
        .map(|tcs| {
            tcs.iter()
                .map(|tc| FunctionCall {
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let content = {
        let t = body.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    };
    ChatCompletionRequestMessage {
        role: m.role.clone(),
        content,
        name: m.name.clone(),
        function_call: None,
        tool_calls,
        refusal: None,
    }
}

/// 使用与消息计数相同的模型回退顺序估算任意序列化文本（如 tools JSON）的 token 数。
///
/// 返回实际采用的 tokenizer 模型；所有已知回退都不可用时按保守的 `3 bytes/token` 估算。
#[must_use]
pub fn count_serialized_text_tokens_openai_compat(
    configured_model: &str,
    text: &str,
) -> TiktokenPromptTokensSnapshot {
    let model = tiktoken_model_id_for_config_model(configured_model);
    if let Ok(bpe) = bpe_for_model(&model) {
        return TiktokenPromptTokensSnapshot {
            prompt_tokens: bpe
                .encode_ordinary(text)
                .len()
                .min(u32::MAX as usize) as u32,
            tiktoken_model: model,
            ..Default::default()
        };
    }
    TiktokenPromptTokensSnapshot {
        prompt_tokens: text.len().div_ceil(3).min(u32::MAX as usize) as u32,
        tiktoken_model: "conservative-bytes".to_string(),
        ..Default::default()
    }
}

fn try_count_with_model(model: &str, tik_messages: &[ChatCompletionRequestMessage]) -> Option<u32> {
    let m = model.trim();
    if m.is_empty() {
        return None;
    }
    let n = num_tokens_from_messages(m, tik_messages).ok()?;
    Some(n.min(u32::MAX as usize) as u32)
}

/// 供 `GET /status` 等展示：当前配置 `model` 在 tiktoken 中是否可直接计数，否则回落到哪个 id。
#[must_use]
pub fn tiktoken_model_id_for_config_model(configured_model: &str) -> String {
    let ping = [ping_message()];
    let trimmed = configured_model.trim();
    if !trimmed.is_empty() && try_count_with_model(trimmed, &ping).is_some() {
        return trimmed.to_string();
    }
    for fallback in ["gpt-4", "gpt-4o"] {
        if try_count_with_model(fallback, &ping).is_some() {
            return fallback.to_string();
        }
    }
    "gpt-4".to_string()
}

/// 对**已**与供应商出站规则对齐的 `messages`（见 [`conversation_messages_to_vendor_body`]）计数。
pub fn count_prompt_tokens_openai_compat_vendor_slice(
    configured_model: &str,
    vendor_messages: &[Message],
) -> Option<TiktokenPromptTokensSnapshot> {
    let tik_messages: Vec<ChatCompletionRequestMessage> = vendor_messages
        .iter()
        .map(crabmate_message_to_tiktoken)
        .collect();
    let trimmed = configured_model.trim();
    let mut candidates: Vec<String> = Vec::new();
    if !trimmed.is_empty() {
        candidates.push(trimmed.to_string());
    }
    for c in ["gpt-4", "gpt-4o"] {
        if !candidates.iter().any(|x| x == c) {
            candidates.push(c.to_string());
        }
    }
    for c in &candidates {
        if let Some(n) = try_count_with_model(c, &tik_messages) {
            return Some(TiktokenPromptTokensSnapshot {
                prompt_tokens: n,
                tiktoken_model: c.clone(),
                ..Default::default()
            });
        }
    }
    None
}

/// 新会话首条消息（`system` + 可选 L6 工作区上下文 `user`，与 [`crate::context_bootstrap::conversation_turn_bootstrap::new_session_prompt_baseline_messages`] 一致）的 prompt token 粗估。
pub fn prompt_token_count_new_session_baseline(
    cfg: &AgentConfig,
    baseline_messages: &[Message],
) -> Option<TiktokenPromptTokensSnapshot> {
    prompt_token_count_vendor_shaped_for_session(cfg, baseline_messages)
}

/// 新会话仅含一条 `system`（L3+L4）时的 prompt token 粗估（不含 L6；优先用 [`prompt_token_count_new_session_baseline`]）。
pub fn prompt_token_count_new_session_system_only_baseline(
    cfg: &AgentConfig,
    system_for_turn: &str,
) -> Option<TiktokenPromptTokensSnapshot> {
    prompt_token_count_vendor_shaped_for_session(
        cfg,
        &[Message::system_only(system_for_turn.to_string())],
    )
}

/// 从**会话内存态** `messages` 出发：先按当前 [`AgentConfig`] 做供应商出站切片，再 tiktoken 计数。
pub fn prompt_token_count_vendor_shaped_for_session(
    cfg: &AgentConfig,
    session_messages: &[Message],
) -> Option<TiktokenPromptTokensSnapshot> {
    let llm_cfg = crate::cm_types::llm_config::LlmConfig {
        llm: cfg.llm.clone(),
        sampling: cfg.llm_sampling.clone(),
        vendor_flags: cfg.llm_vendor_flags.clone(),
        http_retry: cfg.llm_http_retry.clone(),
    };
    let v = llm_vendor_adapter(&cfg.llm.model, &cfg.llm.api_base);
    let vendor = conversation_messages_to_vendor_body(
        session_messages,
        fold_system_into_user_for_config(&cfg.llm.model, &cfg.llm.api_base),
        v.preserve_assistant_tool_call_reasoning(&llm_cfg),
        deepseek_json_output_eligible(&cfg.llm.api_base),
    );
    let snapshot = count_prompt_tokens_openai_compat_vendor_slice(&cfg.llm.model, &vendor)?;
    Some(enrich_snapshot_from_latest_model_context_artifact(
        snapshot,
        session_messages,
    ))
}

fn enrich_snapshot_from_latest_model_context_artifact(
    mut snapshot: TiktokenPromptTokensSnapshot,
    session_messages: &[Message],
) -> TiktokenPromptTokensSnapshot {
    let Some(artifact) =
        crate::agent::model_context_view::artifacts_from_messages(session_messages)
            .into_iter()
            .next_back()
    else {
        return snapshot;
    };
    let report = artifact.compaction;
    if report.after.counting_source.is_none() {
        return snapshot;
    }
    snapshot.prompt_tokens = report.after.message_tokens;
    snapshot.used_input_tokens = Some(
        report
            .provider_input_tokens
            .and_then(|tokens| u32::try_from(tokens).ok())
            .unwrap_or(report.after.used_input_tokens),
    );
    snapshot.max_input_tokens = report.budget.map(|budget| budget.max_input_tokens);
    snapshot.reserved_output_tokens =
        report.budget.map(|budget| budget.reserved_output_tokens);
    snapshot.message_tokens = Some(report.after.message_tokens);
    snapshot.tool_schema_tokens = Some(report.after.tool_schema_tokens);
    snapshot.attachment_tokens = Some(report.after.attachment_tokens);
    snapshot.counting_source = report
        .after
        .counting_source
        .map(|source| source.as_str().to_string());
    snapshot.provider_input_tokens = report.provider_input_tokens;
    if report.provider_input_tokens.is_some() {
        snapshot.counting_source = Some("provider_usage".to_string());
    }
    snapshot
}

/// 单次 LLM 往返的 prompt + completion Token 粗估（供 [`crate::agent::turn_budget::TurnBudgetCounter`] 累计）。
pub fn estimate_chat_exchange_tokens(
    cfg: &AgentConfig,
    request_messages: &[Message],
    response: &Message,
) -> Option<usize> {
    let prompt = prompt_token_count_vendor_shaped_for_session(cfg, request_messages)?;
    let completion = count_prompt_tokens_openai_compat_vendor_slice(
        &cfg.llm.model,
        std::slice::from_ref(response),
    )?;
    Some(prompt.prompt_tokens as usize + completion.prompt_tokens as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    #[test]
    fn vendor_user_only_has_positive_tokens() {
        let msgs = vec![Message::user_only("hello world")];
        let snap = count_prompt_tokens_openai_compat_vendor_slice("gpt-4", &msgs)
            .expect("gpt-4 tokenizer must work in unit tests");
        assert!(snap.prompt_tokens > 0);
        assert!(snap.prompt_tokens < 64);
        assert_eq!(snap.tiktoken_model, "gpt-4");
    }

    #[test]
    fn new_session_system_baseline_positive() {
        let msgs = vec![Message::system_only(
            "You are a helpful assistant.".to_string(),
        )];
        let snap = count_prompt_tokens_openai_compat_vendor_slice("gpt-4", &msgs)
            .expect("gpt-4 tokenizer must work in unit tests");
        assert!(snap.prompt_tokens > 0);
    }

    #[test]
    fn unknown_model_falls_back() {
        let msgs = vec![Message::user_only("x")];
        let snap =
            count_prompt_tokens_openai_compat_vendor_slice("some-vendor-unknown-model-xyz", &msgs)
                .expect("fallback tokenizer");
        assert!(snap.prompt_tokens > 0);
        assert!(snap.tiktoken_model == "gpt-4" || snap.tiktoken_model == "gpt-4o");
    }

    #[test]
    fn latest_model_context_artifact_unifies_budget_and_provider_usage() {
        let cfg = crate::config::load_config(None).expect("default config");
        let report = crate::agent::context_compaction::ContextCompactionReport {
            after: crate::agent::context_compaction::ContextTokenEstimate {
                used_input_tokens: 900,
                message_tokens: 700,
                tool_schema_tokens: 150,
                attachment_tokens: 18,
                vendor_overhead_tokens: 32,
                counting_source: Some(
                    crate::agent::context_compaction::ContextTokenCountingSource::MatchedTokenizer,
                ),
            },
            budget: Some(crate::agent::context_compaction::ContextTokenBudget {
                context_window_tokens: 4_096,
                reserved_output_tokens: 512,
                safety_margin_tokens: 128,
                max_input_tokens: 3_456,
                trigger_tokens: 2_937,
                target_tokens: 2_419,
            }),
            provider_input_tokens: Some(920),
            ..Default::default()
        };
        let artifact = crate::agent::model_context_view::ModelContextArtifact::capture(
            1,
            2,
            &[Message::user_only("hello")],
            report,
            None,
        );
        let messages = vec![
            Message::system_only("system"),
            artifact.into_marker().expect("artifact marker"),
            Message::user_only("hello"),
        ];

        let snapshot = prompt_token_count_vendor_shaped_for_session(&cfg, &messages)
            .expect("token snapshot");
        assert_eq!(snapshot.used_input_tokens, Some(920));
        assert_eq!(snapshot.max_input_tokens, Some(3_456));
        assert_eq!(snapshot.tool_schema_tokens, Some(150));
        assert_eq!(snapshot.counting_source.as_deref(), Some("provider_usage"));
    }
}
