//! 回合输入用量的权威形状（#1347 S3a）。
//!
//! **单一产点**：`agent::context_compaction` 的 `impl ContextCompactionReport::input_usage()`。
//! 对外出口（`context_trim.detail`、SSE `tiktokenPromptTokens`、以及后续的 `GET /status`）
//! 都从 [`TurnInputUsage`] 取值，不再各自从报告里挑字段。
//!
//! **两条口径并列，互不覆写**（这是 S3 的核心修正）：
//!
//! - [`TurnInputUsage::estimated`]：tiktoken 估算的最终请求输入（含 tool schema / 附件 /
//!   vendor 开销）。「本回合用了多少」只认它。
//! - [`TurnInputUsage::provider_input_tokens`]：上游 `usage` 回填的输入 Token（计费口径），
//!   可能缺失。
//!
//! 类型化之前，SSE 侧把 `provider_input_tokens.unwrap_or(estimated)` 写进同一个
//! `used_input_tokens`，再靠 `counting_source = "provider_usage"` 字符串告诉消费方「这次是计费口径」。
//! 那让一个字段承担两种语义，Client 的用量水位在两种口径间会跳变。
//!
//! 设计背景见 `docs/design/context_usage_single_source.md`。

/// 本次估算使用的来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
///
/// 纯数据。**推导**（`from_config`）不在这里：它依赖 `cm_config::context_gates`，
/// 而 `cm_types` 在依赖链更下层，见 `agent::context_compaction::ContextTokenBudget::from_config`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ContextTokenBudget {
    pub context_window_tokens: u32,
    pub reserved_output_tokens: u32,
    pub safety_margin_tokens: u32,
    pub max_input_tokens: u32,
    pub trigger_tokens: u32,
    pub target_tokens: u32,
}

/// 一回合的输入用量：估算与计费两条口径并列。
///
/// **不含「压缩前水位」**：`ContextCompactionReport.before` 是压缩前的诊断量（用来看这一轮
/// 压掉了多少），不属于「本回合向模型发了多少」，所以留在报告里，由
/// `context_trim.detail.before_tokens` 单独透出。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TurnInputUsage {
    /// 视图口径（唯一「本回合用了多少」）。永不被 provider 值覆写。
    pub estimated: ContextTokenEstimate,
    /// 计费口径：上游 `usage` 回填，可缺省。
    pub provider_input_tokens: Option<u64>,
    /// 本回合预算；未配置窗口（`llm_context_tokens == 0`）时为 `None`。
    pub budget: Option<ContextTokenBudget>,
}

impl TurnInputUsage {
    /// 权威用法量（估算口径）。所有「显示用了多少」的出口都应该用这个值。
    #[must_use]
    pub const fn estimated_used(&self) -> u32 {
        self.estimated.used_input_tokens
    }
}

/// 输入预算的**对外展示子集**（#1347 S3b）：`GET /status?view=shell` 的 `context_budget`。
///
/// 只暴露展示需要的四项——`reserved_output_tokens` / `safety_margin_tokens` 已隐含在
/// `max_input_tokens` 里，不进契约面（否则以后调公式还要改 Client）。
///
/// 放在 `cm_types` 而不是 `cm_api_contract` 的原因：推导需要 `cm_config`（`context_gates`），
/// 而 `cm_api_contract` 必须保持「纯 serde、不拉配置加载」（前端 WASM 也编译它）。
///
/// 与 `StatusShellView` 一样是**响应体**：不 `deny_unknown_fields`——以后给预算加字段时，
/// 未升级的 Client 应当忽略掉而不是解析失败。
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(default)]
pub struct ContextBudgetView {
    pub context_window_tokens: u32,
    pub max_input_tokens: u32,
    pub trigger_tokens: u32,
    pub target_tokens: u32,
}

impl ContextBudgetView {
    /// 由预算分段推导结果构造（`cm_config::context_gates`）。
    #[must_use]
    pub const fn from_parts(parts: (u32, u32, u32, u32)) -> Self {
        let (context_window_tokens, max_input_tokens, trigger_tokens, target_tokens) = parts;
        Self {
            context_window_tokens,
            max_input_tokens,
            trigger_tokens,
            target_tokens,
        }
    }
}
