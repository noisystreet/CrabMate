//! 最终请求形状的 Token 预算与完整交互组压缩。
//!
//! 本模块只派生模型视图所需的计量与确定性删组；LLM 摘要仍由 `context_window` 编排。

use crate::agent::message_pipeline::{
    conversation_messages_to_vendor_body, conversation_turn_groups, remove_oldest_turn_group,
};
use crate::agent::tiktoken_prompt_tokens::{
    count_prompt_tokens_openai_compat_vendor_slice, count_serialized_text_tokens_openai_compat,
};
use crate::config::AgentConfig;
use crate::llm::{
    fold_system_into_user_for_config, llm_vendor_adapter, vendor::deepseek_json_output_eligible,
};
use crate::types::{Message, MessageContent, Tool};

const MIN_RECENT_TURN_GROUPS: usize = 2;
const IMAGE_TOKEN_ESTIMATE: u32 = 1_024;
const VENDOR_REQUEST_OVERHEAD_TOKENS: u32 = 32;

/// 本次估算使用的来源。
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextTokenCountingSource {
    MatchedTokenizer,
    FallbackTokenizer,
    ConservativeBytes,
}

impl ContextTokenCountingSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MatchedTokenizer => "matched_tokenizer",
            Self::FallbackTokenizer => "fallback_tokenizer",
            Self::ConservativeBytes => "conservative_bytes",
        }
    }
}

/// 最终请求输入的分项 Token 估算。
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
#[serde(default)]
pub struct ContextTokenEstimate {
    pub used_input_tokens: u32,
    pub message_tokens: u32,
    pub tool_schema_tokens: u32,
    pub attachment_tokens: u32,
    pub vendor_overhead_tokens: u32,
    pub counting_source: Option<ContextTokenCountingSource>,
}

/// 从完整上下文窗口扣除输出预留与安全余量后的输入预算。
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
#[serde(default)]
pub struct ContextTokenBudget {
    pub context_window_tokens: u32,
    pub reserved_output_tokens: u32,
    pub safety_margin_tokens: u32,
    pub max_input_tokens: u32,
    pub trigger_tokens: u32,
    pub target_tokens: u32,
}

impl ContextTokenBudget {
    #[must_use]
    pub fn from_config(cfg: &AgentConfig) -> Option<Self> {
        let context_window_tokens = cfg.llm_sampling.llm_context_tokens;
        if context_window_tokens == 0 {
            return None;
        }
        let reserved_output_tokens = cfg
            .llm_sampling
            .max_tokens
            .min(context_window_tokens.saturating_sub(256));
        let available_after_output =
            context_window_tokens.saturating_sub(reserved_output_tokens);
        let safety_margin_tokens = cfg
            .context_pipeline
            .context_token_safety_margin_tokens
            .min(available_after_output.saturating_sub(128));
        let max_input_tokens = available_after_output.saturating_sub(safety_margin_tokens);
        Some(Self {
            context_window_tokens,
            reserved_output_tokens,
            safety_margin_tokens,
            max_input_tokens,
            trigger_tokens: max_input_tokens
                .saturating_mul(cfg.context_pipeline.context_token_trigger_percent)
                / 100,
            target_tokens: max_input_tokens
                .saturating_mul(cfg.context_pipeline.context_token_target_percent)
                / 100,
        })
    }
}

/// 一次模型视图压缩的可观测报告。
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
#[serde(default)]
pub struct ContextCompactionReport {
    pub budget: Option<ContextTokenBudget>,
    pub before: ContextTokenEstimate,
    pub after: ContextTokenEstimate,
    pub removed_turn_groups: usize,
    pub removed_messages: usize,
    pub token_triggered: bool,
    pub summarized_for_token_budget: bool,
    /// 上游响应 usage 的输入 Token；请求结束后补齐，用于估算校准。
    pub provider_input_tokens: Option<u64>,
}

impl ContextCompactionReport {
    #[must_use]
    pub const fn removed_history(self) -> bool {
        self.removed_turn_groups > 0 || self.summarized_for_token_budget
    }

    #[must_use]
    pub const fn compaction_reason(self) -> &'static str {
        if self.summarized_for_token_budget {
            "token_budget_summary"
        } else if self.removed_turn_groups > 0 {
            "token_budget_turn_groups"
        } else if self.token_triggered {
            "token_budget_warning"
        } else {
            "none"
        }
    }
}

fn attachment_token_estimate(messages: &[Message]) -> u32 {
    let count = messages
        .iter()
        .filter_map(|message| match &message.content {
            Some(MessageContent::Parts(parts)) => Some(parts),
            _ => None,
        })
        .flatten()
        .filter(|part| {
            part.get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind == "image_url")
        })
        .count();
    (count.min(u32::MAX as usize) as u32).saturating_mul(IMAGE_TOKEN_ESTIMATE)
}

/// 按供应商出站 messages、实际工具 schema 与附件估算最终输入 Token。
#[must_use]
pub fn estimate_final_request_tokens(
    cfg: &AgentConfig,
    messages: &[Message],
    tools: &[Tool],
    model_override: Option<&str>,
) -> ContextTokenEstimate {
    let effective_model = model_override.unwrap_or(&cfg.llm.model);
    let llm_cfg = crate::cm_types::llm_config::LlmConfig {
        llm: cfg.llm.clone(),
        sampling: cfg.llm_sampling.clone(),
        vendor_flags: cfg.llm_vendor_flags.clone(),
        http_retry: cfg.llm_http_retry.clone(),
    };
    let adapter = llm_vendor_adapter(&cfg.llm.model, &cfg.llm.api_base);
    let vendor_messages = conversation_messages_to_vendor_body(
        messages,
        fold_system_into_user_for_config(&cfg.llm.model, &cfg.llm.api_base),
        adapter.preserve_assistant_tool_call_reasoning(&llm_cfg),
        deepseek_json_output_eligible(&cfg.llm.api_base),
    );
    let (message_tokens, message_model, message_counting_source) =
        match count_prompt_tokens_openai_compat_vendor_slice(effective_model, &vendor_messages) {
            Some(snapshot) => {
                let source = if snapshot.tiktoken_model == effective_model {
                    ContextTokenCountingSource::MatchedTokenizer
                } else {
                    ContextTokenCountingSource::FallbackTokenizer
                };
                (snapshot.prompt_tokens, snapshot.tiktoken_model, source)
            }
            None => {
                let bytes = serde_json::to_vec(&vendor_messages)
                    .map(|body| body.len())
                    .unwrap_or_default();
                (
                    bytes.div_ceil(3).min(u32::MAX as usize) as u32,
                    "conservative-bytes".to_string(),
                    ContextTokenCountingSource::ConservativeBytes,
                )
            }
        };
    let tool_schema_tokens = if tools.is_empty() {
        0
    } else {
        serde_json::to_string(tools)
            .ok()
            .map(|json| count_serialized_text_tokens_openai_compat(&message_model, &json).prompt_tokens)
            .unwrap_or(0)
    };
    let attachment_tokens = attachment_token_estimate(&vendor_messages);
    let vendor_overhead_tokens = VENDOR_REQUEST_OVERHEAD_TOKENS;
    ContextTokenEstimate {
        used_input_tokens: message_tokens
            .saturating_add(tool_schema_tokens)
            .saturating_add(attachment_tokens)
            .saturating_add(vendor_overhead_tokens),
        message_tokens,
        tool_schema_tokens,
        attachment_tokens,
        vendor_overhead_tokens,
        counting_source: Some(message_counting_source),
    }
}

/// 超过触发阈值时，按最旧完整交互组删除到目标值；始终保留最近两个组。
pub fn compact_messages_to_token_budget(
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    tools: &[Tool],
    model_override: Option<&str>,
    before_summary: ContextTokenEstimate,
    summarized_for_token_budget: bool,
) -> ContextCompactionReport {
    let budget = ContextTokenBudget::from_config(cfg);
    let mut after = estimate_final_request_tokens(cfg, messages, tools, model_override);
    let token_triggered = budget.is_some_and(|value| {
        before_summary.used_input_tokens > value.trigger_tokens
            || after.used_input_tokens > value.trigger_tokens
    });
    let mut removed_turn_groups = 0usize;
    let mut removed_messages = 0usize;
    if let Some(value) = budget
        && token_triggered
    {
        while after.used_input_tokens > value.target_tokens
            && conversation_turn_groups(messages).len() > MIN_RECENT_TURN_GROUPS
        {
            let removed = remove_oldest_turn_group(messages, MIN_RECENT_TURN_GROUPS);
            if removed == 0 {
                break;
            }
            removed_turn_groups = removed_turn_groups.saturating_add(1);
            removed_messages = removed_messages.saturating_add(removed);
            after = estimate_final_request_tokens(cfg, messages, tools, model_override);
        }
    }
    ContextCompactionReport {
        budget,
        before: before_summary,
        after,
        removed_turn_groups,
        removed_messages,
        token_triggered,
        summarized_for_token_budget,
        provider_input_tokens: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionDef, Tool};

    fn tool(name: &str, description_len: usize) -> Tool {
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: name.to_string(),
                description: "d".repeat(description_len),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}}
                }),
            },
        }
    }

    #[test]
    fn tool_schema_and_attachments_are_counted() {
        let cfg = crate::config::load_config(None).expect("embedded config");
        let plain = vec![Message::user_only("hello")];
        let with_image = vec![Message {
            role: "user".to_string(),
            content: Some(MessageContent::Parts(vec![
                serde_json::json!({"type": "text", "text": "hello"}),
                serde_json::json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,AA=="}}),
            ])),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }];
        let no_tools = estimate_final_request_tokens(&cfg, &plain, &[], None);
        let rich = estimate_final_request_tokens(&cfg, &with_image, &[tool("read_file", 200)], None);
        assert!(rich.tool_schema_tokens > 0);
        assert_eq!(rich.attachment_tokens, IMAGE_TOKEN_ESTIMATE);
        assert!(rich.used_input_tokens > no_tools.used_input_tokens);
    }

    #[test]
    fn safe_input_budget_reserves_output_and_configured_margin() {
        let cfg = crate::config::load_config(None).expect("embedded config");
        let budget = ContextTokenBudget::from_config(&cfg).expect("token budget enabled");
        assert_eq!(budget.reserved_output_tokens, 4_096);
        assert_eq!(budget.safety_margin_tokens, 2_048);
        assert_eq!(budget.max_input_tokens, 59_392);
        assert_eq!(budget.trigger_tokens, 50_483);
        assert_eq!(budget.target_tokens, 41_574);
    }

    #[test]
    fn token_compaction_removes_whole_old_groups_and_keeps_recent_groups() {
        let mut cfg = crate::config::load_config(None).expect("embedded config");
        cfg.llm_sampling.llm_context_tokens = 4_096;
        cfg.llm_sampling.max_tokens = 512;
        let mut messages = vec![Message::system_only("system")];
        for turn in 0..4 {
            messages.push(Message::user_only(format!("turn-{turn} {}", "x".repeat(8_000))));
            messages.push(Message::assistant_only(format!("answer-{turn}")));
        }
        let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
        let report =
            compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, false);
        assert!(report.removed_turn_groups > 0);
        assert_eq!(conversation_turn_groups(&messages).len(), 2);
        assert!(
            messages.iter().any(|message| {
                crate::types::message_content_as_str(&message.content)
                    .is_some_and(|content| content.starts_with("turn-3"))
            }),
            "latest user group must remain"
        );
    }

    #[test]
    fn small_tool_dense_history_does_not_compact_by_message_count() {
        let cfg = crate::config::load_config(None).expect("embedded config");
        let mut messages = vec![Message::system_only("system")];
        for turn in 0..4 {
            messages.push(Message::user_only(format!("turn-{turn}")));
            messages.push(Message::assistant_only("calling tool"));
            messages.push(Message {
                role: "tool".to_string(),
                content: Some("ok".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some("read_file".to_string()),
                tool_call_id: Some(format!("call-{turn}")),
            });
            messages.push(Message::assistant_only("done"));
        }
        let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
        let report =
            compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, false);
        assert_eq!(report.removed_turn_groups, 0);
        assert_eq!(conversation_turn_groups(&messages).len(), 4);
    }
}
