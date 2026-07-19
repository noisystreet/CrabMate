//! 使用 **tiktoken-rs** 对「与 [`crate::agent::message_pipeline::conversation_messages_to_vendor_body`] 一致」的
//! `messages` 做 **OpenAI Chat Completions** 风格的 prompt token 近似计数（不含 `tools` JSON、不含图片 token 细项）。
//!
//! 未知 `model` id 时按 **`gpt-4` → `gpt-4o`** 顺序回落，以便 DeepSeek / Kimi 等 OpenAI 兼容网关仍能给出**可比**的粗估值（与真实网关分词可能仍有偏差，见 API 字段说明）。

use tiktoken_rs::{ChatCompletionRequestMessage, FunctionCall, num_tokens_from_messages};

use crate::agent::message_pipeline::conversation_messages_to_vendor_body;
use crate::config::AgentConfig;
use crate::llm::{
    fold_system_into_user_for_config, llm_vendor_adapter, vendor::deepseek_json_output_eligible,
};
use crate::types::{Message, message_content_into_text_lossy};

pub use crabmate_types::TiktokenPromptTokensSnapshot;

fn ping_message() -> ChatCompletionRequestMessage {
    ChatCompletionRequestMessage {
        role: "user".to_string(),
        content: Some("ping".to_string()),
        ..Default::default()
    }
}

fn crabmate_message_to_tiktoken(m: &Message) -> ChatCompletionRequestMessage {
    let mut body = message_content_into_text_lossy(m.content.clone());
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
    let llm_cfg = crabmate_types::llm_config::LlmConfig {
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
    count_prompt_tokens_openai_compat_vendor_slice(&cfg.llm.model, &vendor)
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
}
