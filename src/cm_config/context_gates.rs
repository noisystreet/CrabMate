//! 上下文「双闸」的等效水位推导与自检（#1342）。
//!
//! 两道闸：`max_message_history`（按条数裁剪）与 token 预算（`trigger_tokens`）。
//! 不变量：`条数闸等效 token 水位 ≥ trigger_tokens`。违反时条数闸会先动手，
//! 表现为「上下文水位还很低就开始丢历史」（#1339）。
//!
//! [`ContextTokenBudgetParts`] 同时是 token 预算分段推导的**唯一实现**：
//! 运行期预算（`agent::context_compaction::ContextTokenBudget::from_config`）、
//! 配置自检与 `crabmate doctor` 都从这里取数，避免公式各写一遍后静默分叉。

use crate::cm_config::AgentConfig;

/// 条数闸等效水位用的保守假定（token/条）。
///
/// 依据：工具型 ReAct 会话实测 1,200~1,650 token/条（#1339 的三组实测）。
/// **只用于告警与诊断展示**，不参与任何裁剪判定，也不用于自动调参。
pub const ASSUMED_TOKENS_PER_MESSAGE: u64 = 1_200;

/// `max_message_history` 的取值上界，与 `cm_config::validate` 的同名行单一同源（#1342）。
///
/// 存在的意义：自检给出的「建议条数」在超大窗口下会**超出**这个上界，用户照着调会被校验
/// 直接拒绝。告警必须能识别这种「建议值不可达」的情形，改指另一条路（调低触发百分比）。
pub const MAX_MESSAGE_HISTORY_LIMIT: u64 = 1_024;

/// 最终请求输入预算的分段推导结果。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContextTokenBudgetParts {
    pub context_window_tokens: u32,
    pub reserved_output_tokens: u32,
    pub safety_margin_tokens: u32,
    pub max_input_tokens: u32,
    pub trigger_tokens: u32,
    pub target_tokens: u32,
}

impl ContextTokenBudgetParts {
    /// 由完整窗口与百分比/余量参数推导；`context_window_tokens == 0` 时返回 `None`
    /// （调用方应走 `context_char_budget` 降级路径）。
    #[must_use]
    pub fn derive(
        context_window_tokens: u32,
        max_tokens: u32,
        trigger_percent: u32,
        target_percent: u32,
        safety_margin_tokens: u32,
    ) -> Option<Self> {
        if context_window_tokens == 0 {
            return None;
        }
        let reserved_output_tokens = max_tokens.min(context_window_tokens.saturating_sub(256));
        let available_after_output = context_window_tokens.saturating_sub(reserved_output_tokens);
        let safety_margin_tokens =
            safety_margin_tokens.min(available_after_output.saturating_sub(128));
        let max_input_tokens = available_after_output.saturating_sub(safety_margin_tokens);
        Some(Self {
            context_window_tokens,
            reserved_output_tokens,
            safety_margin_tokens,
            max_input_tokens,
            trigger_tokens: max_input_tokens.saturating_mul(trigger_percent) / 100,
            target_tokens: max_input_tokens.saturating_mul(target_percent) / 100,
        })
    }

    /// 按 `AgentConfig` 的既有字段推导。
    #[must_use]
    pub fn from_config(cfg: &AgentConfig) -> Option<Self> {
        Self::derive(
            cfg.llm_sampling.llm_context_tokens,
            cfg.llm_sampling.max_tokens,
            cfg.context_pipeline.context_token_trigger_percent,
            cfg.context_pipeline.context_token_target_percent,
            cfg.context_pipeline.context_token_safety_margin_tokens,
        )
    }
}

/// 两道闸的等效水位对照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextGateAlignment {
    pub budget: ContextTokenBudgetParts,
    pub max_message_history: usize,
    pub assumed_tokens_per_message: u64,
    /// 条数闸的等效 token 水位。
    pub count_gate_tokens: u64,
}

impl ContextGateAlignment {
    /// 由预算与条数上限组装（不做任何 I/O，便于测试）。
    #[must_use]
    pub fn evaluate(budget: ContextTokenBudgetParts, max_message_history: usize) -> Self {
        let count_gate_tokens =
            u64::try_from(max_message_history).unwrap_or(u64::MAX).saturating_mul(ASSUMED_TOKENS_PER_MESSAGE);
        Self {
            budget,
            max_message_history,
            assumed_tokens_per_message: ASSUMED_TOKENS_PER_MESSAGE,
            count_gate_tokens,
        }
    }

    /// 条数闸是否先于 token 闸动手（即不变量被违反）。
    #[must_use]
    pub fn count_gate_wins(&self) -> bool {
        self.count_gate_tokens < u64::from(self.budget.trigger_tokens)
    }

    /// 让条数闸恰好不早于 token 闸所需的最小条数（向上取整，至少 1）。
    #[must_use]
    pub fn suggested_max_message_history(&self) -> usize {
        let per = self.assumed_tokens_per_message.max(1);
        let n = u64::from(self.budget.trigger_tokens).div_ceil(per).max(1);
        usize::try_from(n).unwrap_or(usize::MAX)
    }
}

/// 按配置算两道闸的对照；token 预算未启用（`llm_context_tokens == 0`）时返回 `None`。
#[must_use]
pub fn context_gate_alignment(cfg: &AgentConfig) -> Option<ContextGateAlignment> {
    let budget = ContextTokenBudgetParts::from_config(cfg)?;
    Some(ContextGateAlignment::evaluate(
        budget,
        cfg.session_ui.max_message_history,
    ))
}

/// 不变量被违反时的告警文案；满足不变量或未启用 token 预算时返回 `None`。
///
/// 返回文案而不是直接打日志，是为了让测试能断言「两条线的数值都出现」，
/// 不必去抓日志输出。
///
/// 建议条数可能**超出**上界（超大窗口 + 默认 1024 条时会算到 1,400 上下），那时「调大条数」
/// 这条路走不通，文案追加 [`unreachable_suggestion_hint`] 指向另一条路。
#[must_use]
pub fn count_gate_warning(cfg: &AgentConfig) -> Option<String> {
    let alignment = context_gate_alignment(cfg)?;
    if !alignment.count_gate_wins() {
        return None;
    }
    let suggested = alignment.suggested_max_message_history();
    let mut message = format!(
        "配置提示：条数闸会比 token 预算先动手 —— max_message_history={} 条 × 假定 {} token/条 ≈ {} token，\
低于 trigger_tokens={}（llm_context_tokens={}，trigger={}%）。后果：上下文水位还很低就会按条数裁剪历史。\
建议 max_message_history ≥ {}，或调低 context_token_trigger_percent。见 #1339 / #1342。",
        alignment.max_message_history,
        alignment.assumed_tokens_per_message,
        alignment.count_gate_tokens,
        alignment.budget.trigger_tokens,
        alignment.budget.context_window_tokens,
        cfg.context_pipeline.context_token_trigger_percent,
        suggested,
    );
    if let Some(hint) = unreachable_suggestion_hint(suggested) {
        message.push_str(&hint);
    }
    Some(message)
}

/// 「建议条数 > 上界」时的补充提示：此时可执行的动作只剩调阈值。
///
/// `pub` 是为了让 `crabmate doctor` 的双闸块复用同一句文案（诊断与自检不得各写一套）。
#[must_use]
pub fn unreachable_suggestion_hint(suggested: usize) -> Option<String> {
    let suggested = u64::try_from(suggested).ok()?;
    if suggested <= MAX_MESSAGE_HISTORY_LIMIT {
        return None;
    }
    Some(format!(
        "【注意】该建议值 {suggested} 超过 max_message_history 的上界 {MAX_MESSAGE_HISTORY_LIMIT}（配置校验会拒绝），\
条数这条路走不通：请调低 context_token_trigger_percent，或接受条数闸先到。"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1M 窗口 / 16,384 输出预留 / 2,048 余量 / 85% 触发 / 70% 目标 —— 与线上实测值一致。
    fn one_mega_budget() -> ContextTokenBudgetParts {
        ContextTokenBudgetParts::derive(1_000_000, 16_384, 85, 70, 2_048).expect("budget")
    }

    #[test]
    fn derive_matches_live_numbers() {
        let b = one_mega_budget();
        assert_eq!(b.reserved_output_tokens, 16_384);
        assert_eq!(b.safety_margin_tokens, 2_048);
        assert_eq!(b.max_input_tokens, 981_568);
        assert_eq!(b.trigger_tokens, 834_332);
        assert_eq!(b.target_tokens, 687_097);
    }

    #[test]
    fn disabled_budget_is_none() {
        assert!(ContextTokenBudgetParts::derive(0, 16_384, 85, 70, 2_048).is_none());
    }

    /// #1339：256 条时条数闸先动手；1024 条时退回兜底。
    #[test]
    fn count_gate_wins_at_256_and_yields_at_1024() {
        let at_256 = ContextGateAlignment::evaluate(one_mega_budget(), 256);
        assert_eq!(at_256.count_gate_tokens, 307_200);
        assert!(at_256.count_gate_wins());

        let at_1024 = ContextGateAlignment::evaluate(one_mega_budget(), 1024);
        assert_eq!(at_1024.count_gate_tokens, 1_228_800);
        assert!(!at_1024.count_gate_wins());
    }

    #[test]
    fn suggested_history_covers_trigger() {
        let a = ContextGateAlignment::evaluate(one_mega_budget(), 256);
        let suggested = a.suggested_max_message_history();
        assert_eq!(suggested, 696); // ceil(834_332 / 1_200)
        assert!(u64::try_from(suggested).unwrap() * ASSUMED_TOKENS_PER_MESSAGE >= 834_332);
    }

    /// 告警文案必须同时给出两条线的数值，否则读者无法自行判断差多少。
    #[test]
    fn warning_reports_both_gate_values() {
        let cfg = crate::cm_config::load_config(None).expect("embedded default config");
        let mut cfg_256 = cfg.clone();
        cfg_256.session_ui.max_message_history = 256;
        cfg_256.llm_sampling.llm_context_tokens = 1_000_000;
        cfg_256.llm_sampling.max_tokens = 16_384;
        let msg = count_gate_warning(&cfg_256).expect("256 条应触发告警");
        assert!(msg.contains("307200"), "应含条数闸等效水位: {msg}");
        assert!(msg.contains(&cfg_256_trigger(&cfg_256).to_string()), "应含 trigger_tokens: {msg}");

        let mut cfg_1024 = cfg_256.clone();
        cfg_1024.session_ui.max_message_history = 1024;
        assert!(count_gate_warning(&cfg_1024).is_none());

        let mut cfg_no_budget = cfg_256.clone();
        cfg_no_budget.llm_sampling.llm_context_tokens = 0;
        assert!(count_gate_warning(&cfg_no_budget).is_none());
    }

    fn cfg_256_trigger(cfg: &AgentConfig) -> u32 {
        context_gate_alignment(cfg)
            .expect("alignment")
            .budget
            .trigger_tokens
    }

    /// 一个「窗口够大、条数取上界」的配置：建议条数会超过 `max_message_history` 上界。
    fn two_mega_at_limit_cfg() -> AgentConfig {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        cfg.session_ui.max_message_history =
            usize::try_from(MAX_MESSAGE_HISTORY_LIMIT).expect("limit fits usize");
        cfg.llm_sampling.llm_context_tokens = 2_000_000;
        cfg.llm_sampling.max_tokens = 16_384;
        cfg
    }

    /// 建议条数超出配置上界时，告警必须指出「条数这条路走不通」（#1342 收尾）。
    ///
    /// 否则用户照着一个被 `validate` 拒绝的值去改，只会得到一条无关的校验错误。
    #[test]
    fn warning_flags_unreachable_suggestion_beyond_limit() {
        let cfg = two_mega_at_limit_cfg();
        let suggested = context_gate_alignment(&cfg)
            .expect("alignment")
            .suggested_max_message_history();
        assert!(
            u64::try_from(suggested).expect("usize fits u64") > MAX_MESSAGE_HISTORY_LIMIT,
            "2M 窗口的建议值应超过上界，实际 {suggested}"
        );
        let msg = count_gate_warning(&cfg).expect("应触发告警");
        assert!(msg.contains(&suggested.to_string()), "应含建议值: {msg}");
        assert!(
            msg.contains(&MAX_MESSAGE_HISTORY_LIMIT.to_string()),
            "应含上界数值: {msg}"
        );
        assert!(msg.contains("上界"), "应指出建议值不可达: {msg}");
    }

    /// 建议值仍在范围内时不追加「不可达」提示（否则告警会误导）。
    #[test]
    fn warning_has_no_unreachable_hint_when_suggestion_fits() {
        let cfg = crate::cm_config::load_config(None).expect("embedded default config");
        let mut cfg_small = cfg;
        cfg_small.session_ui.max_message_history = 256;
        cfg_small.llm_sampling.llm_context_tokens = 1_000_000;
        cfg_small.llm_sampling.max_tokens = 16_384;
        let msg = count_gate_warning(&cfg_small).expect("256 条应触发告警");
        assert!(!msg.contains("上界"), "建议值 696 可达成，不该提上界: {msg}");
    }
}
