//! 最终请求形状的 Token 预算与完整交互组压缩。
//!
//! 本模块只派生模型视图所需的计量与确定性删组；LLM 摘要仍由 `context_window` 编排。

use crate::agent::message_pipeline::{
    RemovedPrefix, conversation_messages_to_vendor_body, conversation_turn_groups,
    remove_oldest_turn_group,
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
/// 分级释放 ① 的上限：旧交互组 `tool` 正文二次压缩到的字符数。
///
/// 同步管道只按 `tool_message_max_chars`（默认 32 KiB）压 `tool` 正文，与模型窗口无关；
/// 接近输入预算时先在旧组上再压一档，可在删除整组之前释放大量 Token。
///
/// **约束**：必须**显著小于** `tool_message_max_chars`，否则正文早已被压到该值以下、
/// 本档永不命中（`maybe_compress_tool_message_content` 在未超限时返回 `None`）。
const DEGRADE_TOOL_MAX_CHARS: usize = 2_000;
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
    /// 分级释放 ① 命中的旧 `tool` 正文条数（视图层压缩，不回写 canonical）。
    pub compressed_old_tool_messages: usize,
    /// 上游响应 usage 的输入 Token；请求结束后补齐，用于估算校准。
    pub provider_input_tokens: Option<u64>,
    /// 被移出视图前缀的连续区间（以本次 Token 压缩输入的会话坐标为基准）。
    /// 仅用于提交阶段从 canonical 逐字切片归档，**不**参与 wire 序列化。
    #[serde(skip)]
    pub removed_prefix: RemovedPrefix,
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

/// 最近 `keep_groups` 个完整交互组的起点下标；组数不足时返回 `messages.len()`（无可降级区间）。
fn recent_groups_start(messages: &[Message], keep_groups: usize) -> usize {
    if keep_groups == 0 {
        return 0;
    }
    let groups = conversation_turn_groups(messages);
    if groups.len() <= keep_groups {
        return messages.len();
    }
    groups[groups.len() - keep_groups].start
}

/// 分级释放 ①：把**最近 `keep_groups` 组之外**的 `tool` 正文二次压缩到 `max_chars`。
///
/// 信封形状由 [`crate::cm_tools::tool_result::maybe_compress_tool_message_content`] 处理：
/// `output` 保留首尾采样，`ok` / 错误码 / 截断元数据原样保留，因此关键证据不会丢。
///
/// 属**视图层**优化：调用方持有的是 `ModelContextView` 副本，不得回写 canonical（ADR §3.7）。
fn compress_old_tool_messages(
    messages: &mut [Message],
    keep_groups: usize,
    max_chars: usize,
) -> usize {
    let boundary = recent_groups_start(messages, keep_groups);
    let mut compressed = 0usize;
    for message in messages.iter_mut().take(boundary) {
        if message.role != "tool" {
            continue;
        }
        let Some(MessageContent::Text(text)) = message.content.as_mut() else {
            continue;
        };
        if let Some(next) =
            crate::cm_tools::tool_result::maybe_compress_tool_message_content(text, max_chars)
        {
            *text = next;
            compressed = compressed.saturating_add(1);
        }
    }
    compressed
}

/// 删组阶段的产出：最终 Token 估算 + 删除统计，供 [`compact_messages_to_token_budget`] 组装报告。
#[derive(Clone, Copy, Debug, Default)]
struct GroupRemovalOutcome {
    after: ContextTokenEstimate,
    turn_groups: usize,
    messages: usize,
    prefix: RemovedPrefix,
}

/// 删除最旧完整交互组直到收敛到目标 Token；返回删除统计与最终估算。
///
/// 每次删组后重算一次估算（确定性、有界），因此单独成函数以保持
/// [`compact_messages_to_token_budget`] 的分支复杂度可读。
fn remove_old_groups_to_target(
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    tools: &[Tool],
    model_override: Option<&str>,
    after: ContextTokenEstimate,
    target_tokens: u32,
    min_tail_messages: usize,
) -> GroupRemovalOutcome {
    let mut outcome = GroupRemovalOutcome {
        after,
        ..GroupRemovalOutcome::default()
    };
    while outcome.after.used_input_tokens > target_tokens {
        let groups = conversation_turn_groups(messages);
        if groups.len() <= MIN_RECENT_TURN_GROUPS {
            break;
        }
        // 删掉最旧一组后的尾部消息数；低于摘要保留条数就停手（那是这次摘要刻意留下的）。
        if min_tail_messages > 0
            && messages.len().saturating_sub(groups[1].start) < min_tail_messages
        {
            break;
        }
        let step = remove_oldest_turn_group(messages, MIN_RECENT_TURN_GROUPS);
        if step.is_empty() {
            break;
        }
        outcome.turn_groups = outcome.turn_groups.saturating_add(1);
        outcome.messages = outcome.messages.saturating_add(step.removed);
        outcome.prefix = outcome.prefix.union(step);
        outcome.after = estimate_final_request_tokens(cfg, messages, tools, model_override);
    }
    outcome
}

/// 超过触发阈值时按「分级释放」收敛到目标 Token：
/// ① 旧 `tool` 正文二次压缩 → ② 删除最旧完整交互组（始终保留最近两个组）。
///
/// **不设「剥离旧 reasoning」这一级**：出站路径默认已剥离 `reasoning_content`
/// （`conversation_messages_to_vendor_body`），估算里本就不含旧思维链，剥离没有 Token 收益；
/// 而在启用「工具调用回传思维链」的供应商（Kimi k2.5 thinking / DeepSeek thinking）上剥离
/// 会让上游要求回传的正文变空串，属契约冲突。
///
/// `min_tail_messages` 非零时（本回合做过 LLM 摘要），删组不会吃掉摘要刻意保留的尾部条数，
/// 否则那次摘要等于白烧一次模型调用。
pub fn compact_messages_to_token_budget(
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    tools: &[Tool],
    model_override: Option<&str>,
    before_summary: ContextTokenEstimate,
    summarized_for_token_budget: bool,
    min_tail_messages: usize,
) -> ContextCompactionReport {
    let budget = ContextTokenBudget::from_config(cfg);
    let mut after = estimate_final_request_tokens(cfg, messages, tools, model_override);
    let token_triggered = budget.is_some_and(|value| {
        before_summary.used_input_tokens > value.trigger_tokens
            || after.used_input_tokens > value.trigger_tokens
    });
    let mut compressed_old_tool_messages = 0usize;
    let mut removal = GroupRemovalOutcome {
        after,
        ..GroupRemovalOutcome::default()
    };
    if let Some(value) = budget
        && token_triggered
    {
        if after.used_input_tokens > value.target_tokens {
            compressed_old_tool_messages =
                compress_old_tool_messages(messages, MIN_RECENT_TURN_GROUPS, DEGRADE_TOOL_MAX_CHARS);
            if compressed_old_tool_messages > 0 {
                after = estimate_final_request_tokens(cfg, messages, tools, model_override);
            }
        }
        removal = remove_old_groups_to_target(
            cfg,
            messages,
            tools,
            model_override,
            after,
            value.target_tokens,
            min_tail_messages,
        );
    }
    ContextCompactionReport {
        budget,
        before: before_summary,
        after: removal.after,
        removed_turn_groups: removal.turn_groups,
        removed_messages: removal.messages,
        token_triggered,
        summarized_for_token_budget,
        compressed_old_tool_messages,
        provider_input_tokens: None,
        removed_prefix: removal.prefix,
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
        assert_eq!(budget.reserved_output_tokens, 16_384);
        assert_eq!(budget.safety_margin_tokens, 2_048);
        assert_eq!(budget.max_input_tokens, 243_712);
        assert_eq!(budget.trigger_tokens, 207_155);
        assert_eq!(budget.target_tokens, 170_598);
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
            compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, false, 0);
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
            messages.push(Message::tool_result_named(format!("call-{turn}"), "read_file".to_string(), "ok"));
            messages.push(Message::assistant_only("done"));
        }
        let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
        let report =
            compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, false, 0);
        assert_eq!(report.removed_turn_groups, 0);
        assert_eq!(conversation_turn_groups(&messages).len(), 4);
    }

    /// 生成高熵正文：避免重复字符被 BPE 合并成超长 token，保证 Token 估算与真实体积接近。
    fn bulky_text(tag: &str, lines: usize) -> String {
        (0..lines)
            .map(|i| format!("第{i}行 {tag} 内容 value_{i}\n"))
            .collect()
    }

    fn tool_call_message() -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("calling".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: Some(vec![crate::types::ToolCall {
                id: "call-1".to_string(),
                typ: "function".to_string(),
                function: crate::types::FunctionCall {
                    name: "run_command".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            name: None,
            tool_call_id: None,
        }
    }

    fn tool_message(content: String) -> Message {
        Message::tool_result_named("call-1".to_string(), "run_command".to_string(), content)
    }

    /// 分级释放 ①：只压缩最近两组之外的 `tool` 正文。
    #[test]
    fn degrade_compresses_old_tool_bodies_only() {
        let big = bulky_text("tool", 2_000);
        let mut messages = vec![Message::system_only("system")];
        for turn in 0..3 {
            messages.push(Message::user_only(format!("turn-{turn}")));
            messages.push(tool_call_message());
            messages.push(tool_message(big.clone()));
            messages.push(Message::assistant_only("done"));
        }
        let compressed = compress_old_tool_messages(
            &mut messages,
            MIN_RECENT_TURN_GROUPS,
            DEGRADE_TOOL_MAX_CHARS,
        );
        assert_eq!(compressed, 1, "仅最旧交互组的 tool 正文应被二次压缩");
        let lengths: Vec<usize> = messages
            .iter()
            .filter(|message| message.role == "tool")
            .map(|message| {
                crate::types::message_content_as_str(&message.content)
                    .unwrap_or("")
                    .chars()
                    .count()
            })
            .collect();
        assert_eq!(lengths.len(), 3);
        assert!(lengths[0] < big.chars().count(), "旧组正文应被压缩");
        assert_eq!(lengths[1], big.chars().count(), "最近两组保持原样");
        assert_eq!(lengths[2], big.chars().count(), "最近两组保持原样");
    }

    /// 超预算时优先压旧 `tool` 正文；压缩后达标则不再删组。
    #[test]
    fn token_compaction_prefers_tool_body_degrade_over_group_removal() {
        let mut cfg = crate::config::load_config(None).expect("embedded config");
        cfg.llm_sampling.llm_context_tokens = 16_384;
        cfg.llm_sampling.max_tokens = 512;
        let mut messages = vec![Message::system_only("system")];
        messages.push(Message::user_only("turn-0"));
        messages.push(tool_call_message());
        messages.push(tool_message(bulky_text("tool", 4_000)));
        messages.push(Message::assistant_only("done"));
        for turn in 1..3 {
            messages.push(Message::user_only(format!("turn-{turn}")));
            messages.push(Message::assistant_only("answer"));
        }
        let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
        let report =
            compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, false, 0);
        assert!(report.token_triggered, "巨型 tool 正文应触发 Token 预算");
        assert_eq!(report.compressed_old_tool_messages, 1);
        assert_eq!(report.removed_turn_groups, 0, "压缩旧 tool 正文后应已达标");
        let target = report.budget.expect("token budget enabled").target_tokens;
        assert!(report.after.used_input_tokens <= target);
    }

    /// `min_tail_messages` 非零时（刚做过摘要），删组不得吃掉摘要保留的尾部条数。
    #[test]
    fn min_tail_messages_stops_group_removal() {
        let mut cfg = crate::config::load_config(None).expect("embedded config");
        cfg.llm_sampling.llm_context_tokens = 4_096;
        cfg.llm_sampling.max_tokens = 512;
        let build = || {
            let mut messages = vec![Message::system_only("system")];
            for turn in 0..4 {
                messages.push(Message::user_only(bulky_text(&format!("turn{turn}"), 400)));
                messages.push(Message::assistant_only("answer"));
            }
            messages
        };

        let mut guarded_messages = build();
        let before = estimate_final_request_tokens(&cfg, &guarded_messages, &[], None);
        let guarded = compact_messages_to_token_budget(
            &cfg,
            &mut guarded_messages,
            &[],
            None,
            before,
            false,
            100,
        );
        assert_eq!(guarded.removed_turn_groups, 0, "尾部保护应阻止删组");

        let mut free_messages = build();
        let before = estimate_final_request_tokens(&cfg, &free_messages, &[], None);
        let freed =
            compact_messages_to_token_budget(&cfg, &mut free_messages, &[], None, before, false, 0);
        assert!(freed.removed_turn_groups > 0, "无尾部保护时应删组");
    }

    /// `min_tail_messages` 取生产默认尾部条数（12）时的真实区间：保护优先于继续删组。
    #[test]
    fn min_tail_messages_default_tail_keeps_more_groups_than_no_guard() {
        let mut cfg = crate::config::load_config(None).expect("embedded config");
        cfg.llm_sampling.llm_context_tokens = 4_096;
        cfg.llm_sampling.max_tokens = 512;
        let tail = cfg.context_pipeline.context_summary_tail_messages.clamp(4, 64);
        assert_eq!(tail, 12, "默认配置的摘要尾部条数");
        let build = || {
            let mut messages = vec![Message::system_only("system")];
            for turn in 0..8 {
                messages.push(Message::user_only(bulky_text(&format!("turn{turn}"), 200)));
                messages.push(Message::assistant_only("answer"));
            }
            messages
        };

        let mut guarded_messages = build();
        let before = estimate_final_request_tokens(&cfg, &guarded_messages, &[], None);
        let guarded =
            compact_messages_to_token_budget(&cfg, &mut guarded_messages, &[], None, before, false, tail);

        let mut free_messages = build();
        let before = estimate_final_request_tokens(&cfg, &free_messages, &[], None);
        let freed =
            compact_messages_to_token_budget(&cfg, &mut free_messages, &[], None, before, false, 0);

        assert!(guarded.removed_turn_groups > 0, "预算超限时应至少删一组");
        assert!(
            freed.removed_turn_groups > guarded.removed_turn_groups,
            "无尾部保护应删得更多: guarded={} freed={}",
            guarded.removed_turn_groups,
            freed.removed_turn_groups
        );
        let kept_messages = guarded_messages.len();
        assert!(
            kept_messages >= tail,
            "保护生效后应保留至少 {tail} 条消息: {kept_messages}"
        );
    }
}
