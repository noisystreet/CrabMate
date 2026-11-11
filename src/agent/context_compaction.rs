//! 最终请求形状的 Token 预算与完整交互组压缩。
//!
//! 本模块只派生模型视图所需的计量与确定性删组；LLM 摘要仍由 `context_window` 编排。
//!
//! **决策**（压哪一级、停在哪）在 `policy` 子模块：纯函数返回有界动作序列，本文件只做执行与
//! 逐组重估（#1347 S2）。新增一级释放 = policy 多一个变体 + 本模块多一个执行分支。

use crate::agent::message_pipeline::{
    RemovedPrefix, conversation_messages_to_vendor_body, conversation_turn_groups,
    recent_groups_start, remove_oldest_turn_group,
};
use crate::agent::tiktoken_prompt_tokens::{
    count_prompt_tokens_openai_compat_vendor_slice, count_serialized_text_tokens_openai_compat,
};
use crate::config::AgentConfig;
use crate::llm::{
    fold_system_into_user_for_config, llm_vendor_adapter, vendor::deepseek_json_output_eligible,
};
use crate::types::{Message, MessageContent, Tool};

pub(crate) mod policy;

use policy::{CompactionPlan, CompactionProbe, ContextAction, plan_compaction};

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
    /// 与配置自检、`crabmate doctor` 共用 `cm_config::context_gates` 的**同一份**推导（#1342）：
    /// 公式只写一遍，改阈值语义不会出现「运行期与诊断各算一套」的静默分叉。
    #[must_use]
    pub fn from_config(cfg: &AgentConfig) -> Option<Self> {
        let parts = crate::cm_config::context_gates::ContextTokenBudgetParts::from_config(cfg)?;
        Some(Self {
            context_window_tokens: parts.context_window_tokens,
            reserved_output_tokens: parts.reserved_output_tokens,
            safety_margin_tokens: parts.safety_margin_tokens,
            max_input_tokens: parts.max_input_tokens,
            trigger_tokens: parts.trigger_tokens,
            target_tokens: parts.target_tokens,
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
    /// 单行撮要，供 `RUST_LOG` debug 汇总与排障 grep（**不含** `removed_prefix`，它是 canonical 坐标）。
    /// budget 为 `None`（未配置 `llm_context_tokens`、走字符预算降级路径）时预算字段写 `-`，但
    /// before/after 与分项照常输出——那条路径同样需要观察用量。
    ///
    /// `pub(crate)`：没有外部消费者；两层模块都是 `pub mod`，写成 `pub` 会悄悄扩大 crate 可见面，
    /// 而 `scripts/check-public-api.py` 只扫根级顶格名字，覆盖不到这一层。
    #[must_use]
    pub(crate) fn format_for_log(&self) -> String {
        let budget = self.budget;
        format!(
            "context_window_tokens={} before_tokens={} after_tokens={} max_input_tokens={} \
             trigger_tokens={} target_tokens={} reserved_output_tokens={} safety_margin_tokens={} \
             message_tokens={} tool_schema_tokens={} attachment_tokens={} vendor_overhead_tokens={} \
             removed_turn_groups={} removed_messages={} compressed_old_tool_messages={} \
             token_triggered={} summarized_for_token_budget={} provider_input_tokens={} \
             counting_source={} reason={}",
            opt_num(budget.map(|b| b.context_window_tokens)),
            self.before.used_input_tokens,
            self.after.used_input_tokens,
            opt_num(budget.map(|b| b.max_input_tokens)),
            opt_num(budget.map(|b| b.trigger_tokens)),
            opt_num(budget.map(|b| b.target_tokens)),
            opt_num(budget.map(|b| b.reserved_output_tokens)),
            opt_num(budget.map(|b| b.safety_margin_tokens)),
            self.after.message_tokens,
            self.after.tool_schema_tokens,
            self.after.attachment_tokens,
            self.after.vendor_overhead_tokens,
            self.removed_turn_groups,
            self.removed_messages,
            self.compressed_old_tool_messages,
            self.token_triggered,
            self.summarized_for_token_budget,
            opt_num(self.provider_input_tokens),
            self.after
                .counting_source
                .map_or("unknown", ContextTokenCountingSource::as_str),
            self.compaction_reason(),
        )
    }

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

/// `Some(v)` 打印值、`None` 打印 `-`：debug 行里「未配置」与「值为 0」必须能分辨。
fn opt_num<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_string(), |v| v.to_string())
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

/// 单级删组的三个旋钮：目标水位 + 两个守卫，取值由 [`plan_compaction`] 决定。
///
/// 打包成结构体，一是让执行函数不撞 clippy 的参数上限，二是让「收敛到哪」与
/// 「被什么挡住」在调用点一眼可读。
#[derive(Clone, Copy, Debug)]
struct RemovalStep {
    /// 本级的收敛目标水位。
    target_tokens: u32,
    /// 本回合 LLM 摘要刻意保留的尾部条数；`0` 表示本级不保护尾部。
    tail_guard: usize,
    /// 最少保留几个完整交互组。
    min_groups: usize,
}

/// 单轮删到目标：`step.tail_guard` 为 `0` 时不保护本回合摘要保留的尾部。
///
/// 返回 `true` 表示已收敛到目标水位；`false` 表示停手（只剩 `step.min_groups` 个完整组、
/// 被 `step.tail_guard` 挡住，或已无可删完整组）。
///
/// 「停在哪」的两个守卫都由 [`plan_compaction`] 写进 [`ContextAction::TrimTurnGroupsTo`]，
/// 本函数只做执行与逐组重估（确定性、有界）。
fn remove_groups_until_target(
    outcome: &mut GroupRemovalOutcome,
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    tools: &[Tool],
    model_override: Option<&str>,
    step: RemovalStep,
) -> bool {
    while outcome.after.used_input_tokens > step.target_tokens {
        let groups = conversation_turn_groups(messages);
        if groups.len() <= step.min_groups {
            return false;
        }
        // 删掉最旧一组后的尾部消息数；低于摘要保留条数就停手（那是这次摘要刻意留下的）。
        if step.tail_guard > 0
            && messages.len().saturating_sub(groups[1].start) < step.tail_guard
        {
            return false;
        }
        let removed = remove_oldest_turn_group(messages, step.min_groups);
        if removed.is_empty() {
            return false;
        }
        outcome.turn_groups = outcome.turn_groups.saturating_add(1);
        outcome.messages = outcome.messages.saturating_add(removed.removed);
        outcome.prefix = outcome.prefix.union(removed);
        outcome.after = estimate_final_request_tokens(cfg, messages, tools, model_override);
    }
    true
}

/// 删除最旧完整交互组直到收敛；返回删除统计与最终估算。
///
/// 分两级：
/// 1. 删到**软目标** `soft.target_tokens`，全程尊重 `soft.tail_guard`（本回合 LLM 摘要刻意保留的尾部）；
/// 2. 软目标没达成、但已经会撞**硬上限** `hard_limit_tokens` 时，放开尾部保护删到硬上限。
///
/// 为什么要第 2 级：摘要后的视图只剩「摘要 + 尾部」，第 1 级的守卫恒成立、一次都不会删，
/// 于是**摘要保留的尾部本身超窗口时压缩到此为止**，请求会带着超预算的 messages 发出并被
/// 上游拒绝——那比丢掉「已在 canonical、提交时进归档表」的原文更糟。反过来，软目标只是个
/// 理想值，撞不上窗口就不值得为它牺牲摘要刻意留下的原文，所以第 2 级不按软目标判断。
fn remove_old_groups_to_target(
    cfg: &AgentConfig,
    messages: &mut Vec<Message>,
    tools: &[Tool],
    model_override: Option<&str>,
    after: ContextTokenEstimate,
    soft: RemovalStep,
    hard_limit_tokens: Option<u32>,
) -> GroupRemovalOutcome {
    let mut outcome = GroupRemovalOutcome {
        after,
        ..GroupRemovalOutcome::default()
    };
    if remove_groups_until_target(&mut outcome, cfg, messages, tools, model_override, soft) {
        return outcome;
    }
    if let Some(hard_limit) = hard_limit_tokens
        && outcome.after.used_input_tokens > hard_limit
    {
        let _ = remove_groups_until_target(
            &mut outcome,
            cfg,
            messages,
            tools,
            model_override,
            RemovalStep {
                target_tokens: hard_limit,
                tail_guard: 0,
                min_groups: soft.min_groups,
            },
        );
    }
    outcome
}

/// 超过触发阈值时按「分级释放」收敛到目标 Token：
/// ① 旧 `tool` 正文二次压缩 → ② 删除最旧完整交互组（始终保留最近两个组）。
///
/// **动作序列与守卫参数由 `policy::plan_compaction` 决定**（#1347 S2 的决策单点），本函数只做执行与
/// 逐步重估：降级之后要重新量水位才知道还该不该删组，所以循环留在这一侧、判据留在那一侧。
///
/// **不设「剥离旧 reasoning」这一级**：出站路径默认已剥离 `reasoning_content`
/// （`conversation_messages_to_vendor_body`），估算里本就不含旧思维链，剥离没有 Token 收益；
/// 而在启用「工具调用回传思维链」的供应商（Kimi k2.5 thinking / DeepSeek thinking）上剥离
/// 会让上游要求回传的正文变空串，属契约冲突。
///
/// `min_tail_messages` 非零时（本回合做过 LLM 摘要），删组**优先**不吃掉摘要刻意保留的
/// 尾部条数，否则那次摘要等于白烧一次模型调用；只有尾部本身就会撞输入硬上限时才会放开
/// 这层保护（见 `remove_old_groups_to_target` 的第 2 级）。
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
    let plan = plan_compaction(&CompactionProbe {
        budget,
        before: before_summary,
        current: after,
        min_tail_messages,
    });
    log_plan(&plan);
    let mut compressed_old_tool_messages = 0usize;
    let mut removal = GroupRemovalOutcome {
        after,
        ..GroupRemovalOutcome::default()
    };
    for action in &plan.actions {
        match *action {
            ContextAction::DegradeOldToolBodies {
                keep_groups,
                max_chars,
            } => {
                compressed_old_tool_messages =
                    compress_old_tool_messages(messages, keep_groups, max_chars);
                if compressed_old_tool_messages > 0 {
                    after = estimate_final_request_tokens(cfg, messages, tools, model_override);
                }
                // 报告的水位必须跟着执行走。当前 policy 恒把本动作与 `TrimTurnGroupsTo` 成对给出，
                // 所以 `remove_old_groups_to_target` 随后会用 `after` 覆盖 `removal.after`；
                // 但一旦出现「只降级、不删组」的单体动作（policy 抽出来正是为了让这种组合可加），
                // 少了这一行报告就会停在**降级前**的水位，`context_trim` 与 S3 的权威用量跟着错。
                removal.after = after;
            }
            ContextAction::TrimTurnGroupsTo {
                target_tokens,
                hard_limit_tokens,
                tail_guard,
                min_groups,
            } => {
                removal = remove_old_groups_to_target(
                    cfg,
                    messages,
                    tools,
                    model_override,
                    after,
                    RemovalStep {
                        target_tokens,
                        tail_guard,
                        min_groups,
                    },
                    hard_limit_tokens,
                );
            }
        }
    }
    ContextCompactionReport {
        budget,
        before: before_summary,
        after: removal.after,
        removed_turn_groups: removal.turn_groups,
        removed_messages: removal.messages,
        token_triggered: plan.token_triggered,
        summarized_for_token_budget,
        compressed_old_tool_messages,
        provider_input_tokens: None,
        removed_prefix: removal.prefix,
    }
}

/// 一行 debug：本回合的决策（级数与顺序）。与 `context_window::log_compaction_report`
/// 的执行结果互补——那边说「压掉了多少」，这边说「本来打算压哪几级」。
fn log_plan(plan: &CompactionPlan) {
    if !log::log_enabled!(target: "crabmate::context_compaction", log::Level::Debug) {
        return;
    }
    log::debug!(
        target: "crabmate::context_compaction",
        "compaction_plan token_triggered={} actions={:?}",
        plan.token_triggered,
        plan.kinds(),
    );
}

#[cfg(test)]
mod golden;
#[cfg(test)]
mod tests;
