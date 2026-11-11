//! 上下文压缩的**决策单点**（#1347 S2）。
//!
//! 把「压哪一级、停在哪」从执行路径里剥出来：本模块只做纯判定（无 IO、不碰 `messages`），
//! 返回有界、顺序固定的动作序列；执行与「每删一步重新估算」留在
//! [`super::compact_messages_to_token_budget`] 与 `crate::agent::context_window`。
//!
//! **动作序列为什么不写成「删 N 组」**：删组次数取决于每删一步后重新跑 tiktoken 得到的水位，
//! policy 拿不到未来水位。所以一个动作 = 一条**释放臂**，臂内自带「停在哪」的守卫参数，
//! 由执行器逐步执行、逐步重估；「这个动作没改动任何东西」也是合法结果
//! （例如降级之后水位已达标，`TrimTurnGroupsTo` 一次都不删）。
//!
//! 新增一级释放 = 在本模块多一个 [`ContextAction`] 变体 + 执行器多一个分支，
//! 判据与顺序只在这里写一遍。

use crate::agent::message_pipeline::MIN_RECENT_TURN_GROUPS;

use super::{ContextTokenBudget, ContextTokenEstimate, DEGRADE_TOOL_MAX_CHARS};

/// 一次压缩决策的纯输入。
///
/// 刻意不含 `&mut Vec<Message>`：判据只依赖预算与两个水位，消息本身由执行器持有。
#[derive(Clone, Copy, Debug)]
pub(crate) struct CompactionProbe {
    /// `None` 表示未配置 `llm_context_tokens`，走字符预算降级路径。
    pub budget: Option<ContextTokenBudget>,
    /// 摘要**之前**的水位：与 `current` 一起判 `trigger_tokens`（任一超即触发）。
    pub before: ContextTokenEstimate,
    /// 当前 `messages` 的水位（摘要之后）：判 `target_tokens`。
    pub current: ContextTokenEstimate,
    /// 本回合 LLM 摘要实际保留的尾部条数；`0` 表示本回合未摘要。
    pub min_tail_messages: usize,
}

/// 分级释放的一步。**动作 = 一条释放臂**，不等于「一定发生了改动」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContextAction {
    /// ① 最近 `keep_groups` 组之外的 `tool` 正文二次压缩到 `max_chars`。
    DegradeOldToolBodies { keep_groups: usize, max_chars: usize },
    /// ② 删最旧完整交互组：先到 `target_tokens`（全程尊重 `tail_guard`），
    /// 软目标没达成但撞 `hard_limit_tokens` 时放开尾部保护继续删。
    ///
    /// 两级合成一个动作是有意的：第 2 级的触发条件是「第 1 级没收敛 **且** 仍超硬上限」，
    /// 拆成两个动作就得把控制流重新塞回执行器，等于没搬。
    TrimTurnGroupsTo {
        target_tokens: u32,
        hard_limit_tokens: Option<u32>,
        tail_guard: usize,
        min_groups: usize,
    },
}

impl ContextAction {
    /// 稳定短名（写进 golden 与 debug 行；改名即改断言，属有意的契约变更）。
    #[must_use]
    pub(crate) const fn kind(self) -> &'static str {
        match self {
            Self::DegradeOldToolBodies { .. } => "degrade_old_tool_bodies",
            Self::TrimTurnGroupsTo { .. } => "trim_turn_groups_to",
        }
    }
}

/// [`plan_compaction`] 的产物。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CompactionPlan {
    /// 与 `ContextCompactionReport::token_triggered` 同源：报告不再自己算一遍。
    pub token_triggered: bool,
    /// 顺序即执行顺序；空表示本回合不动手。
    pub actions: Vec<ContextAction>,
}

impl CompactionPlan {
    /// 动作短名序列，供 golden 与 debug 行断言「级数与顺序」。
    #[must_use]
    pub(crate) fn kinds(&self) -> Vec<&'static str> {
        self.actions.iter().map(|action| action.kind()).collect()
    }
}

/// 「压哪一级、停在哪」的唯一判定。
///
/// 四个出口对应行为契约的四类边界（S1 golden 逐个守）：
/// 未配置窗口 / 未触发 / 触发但已在软目标内 / 触发且超软目标。
#[must_use]
pub(crate) fn plan_compaction(probe: &CompactionProbe) -> CompactionPlan {
    let Some(budget) = probe.budget else {
        // 未配置窗口：token 分级一律不动手，压缩交给同步管道的字符预算。
        return CompactionPlan::default();
    };
    let token_triggered = probe.before.used_input_tokens > budget.trigger_tokens
        || probe.current.used_input_tokens > budget.trigger_tokens;
    if !token_triggered || probe.current.used_input_tokens <= budget.target_tokens {
        // 触发但当前已在软目标内：只留触发标记（报告原因为 `token_budget_warning`），不释放。
        return CompactionPlan {
            token_triggered,
            actions: Vec::new(),
        };
    }
    CompactionPlan {
        token_triggered,
        actions: vec![
            ContextAction::DegradeOldToolBodies {
                keep_groups: MIN_RECENT_TURN_GROUPS,
                max_chars: DEGRADE_TOOL_MAX_CHARS,
            },
            ContextAction::TrimTurnGroupsTo {
                target_tokens: budget.target_tokens,
                hard_limit_tokens: Some(budget.max_input_tokens),
                tail_guard: probe.min_tail_messages,
                min_groups: MIN_RECENT_TURN_GROUPS,
            },
        ],
    }
}

/// 硬上限兜底：放开「最近若干组不降级」的默认软保护。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SqueezeAllToolBodies {
    pub max_chars: usize,
    /// `0` = 连最近组一起压（兜底路径），见 [`crate::agent::message_pipeline::recent_groups_start`]。
    pub keep_groups: usize,
}

/// 超硬上限、且本回合**没有**删过组时，才允许连最近组一起压。
///
/// `removed_turn_groups > 0` 不是冗余守卫：删过组说明旧历史已经让过位，再采样掉模型眼前的
/// 最新工具输出，常常换来一次额外的工具往返。
#[must_use]
pub(crate) fn plan_hard_limit_fallback(
    after: ContextTokenEstimate,
    budget: Option<ContextTokenBudget>,
    removed_turn_groups: usize,
    tool_message_max_chars: usize,
) -> Option<SqueezeAllToolBodies> {
    let budget = budget?;
    if after.used_input_tokens <= budget.max_input_tokens || removed_turn_groups > 0 {
        return None;
    }
    Some(SqueezeAllToolBodies {
        max_chars: tool_message_max_chars,
        keep_groups: 0,
    })
}

/// 压缩已尽力仍超硬上限的显式信号。
///
/// 多半是 `llm_context_tokens` 与模型真实窗口不一致：配了窗口后 `context_char_budget` 已停用
/// （见 `context_window::message_pipeline_config_for_turn`），此时没有任何一级还能兜住。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExhaustedWarning {
    pub after_tokens: u32,
    pub max_input_tokens: u32,
    pub context_window_tokens: u32,
}

/// 该不该告警：**只有**超硬上限才告警，与兜底动作的触发条件分开两条阈值。
#[must_use]
pub(crate) fn exhausted_warning(
    after: ContextTokenEstimate,
    budget: Option<ContextTokenBudget>,
) -> Option<ExhaustedWarning> {
    let budget = budget?;
    (after.used_input_tokens > budget.max_input_tokens).then_some(ExhaustedWarning {
        after_tokens: after.used_input_tokens,
        max_input_tokens: budget.max_input_tokens,
        context_window_tokens: budget.context_window_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(trigger: u32, target: u32, max_input: u32) -> Option<ContextTokenBudget> {
        Some(ContextTokenBudget {
            context_window_tokens: max_input,
            max_input_tokens: max_input,
            trigger_tokens: trigger,
            target_tokens: target,
            ..ContextTokenBudget::default()
        })
    }

    fn est(used: u32) -> ContextTokenEstimate {
        ContextTokenEstimate {
            used_input_tokens: used,
            ..ContextTokenEstimate::default()
        }
    }

    fn probe(
        budget: Option<ContextTokenBudget>,
        before: u32,
        current: u32,
        min_tail: usize,
    ) -> CompactionProbe {
        CompactionProbe {
            budget,
            before: est(before),
            current: est(current),
            min_tail_messages: min_tail,
        }
    }

    #[test]
    fn no_window_plans_nothing() {
        let plan = plan_compaction(&probe(None, 1_000_000, 1_000_000, 0));
        assert!(!plan.token_triggered);
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn below_trigger_plans_nothing() {
        let plan = plan_compaction(&probe(budget(100, 80, 120), 50, 50, 0));
        assert!(!plan.token_triggered);
        assert!(plan.actions.is_empty());
    }

    /// 触发但当前已在软目标内：报告原因要留 `token_budget_warning`，但不该有任何释放动作。
    #[test]
    fn triggered_but_within_target_keeps_only_the_flag() {
        let plan = plan_compaction(&probe(budget(100, 100, 120), 150, 90, 0));
        assert!(plan.token_triggered);
        assert!(plan.actions.is_empty());
    }

    /// 摘要前水位超 trigger 就够触发，哪怕摘要后已回落。
    #[test]
    fn before_watermark_alone_triggers() {
        let plan = plan_compaction(&probe(budget(100, 10, 120), 150, 5, 0));
        assert!(plan.token_triggered);
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn triggered_above_target_plans_degrade_then_trim() {
        let plan = plan_compaction(&probe(budget(100, 80, 120), 150, 150, 0));
        assert!(plan.token_triggered);
        assert_eq!(
            plan.kinds(),
            vec!["degrade_old_tool_bodies", "trim_turn_groups_to"]
        );
        assert_eq!(
            plan.actions[0],
            ContextAction::DegradeOldToolBodies {
                keep_groups: MIN_RECENT_TURN_GROUPS,
                max_chars: DEGRADE_TOOL_MAX_CHARS,
            }
        );
        assert_eq!(
            plan.actions[1],
            ContextAction::TrimTurnGroupsTo {
                target_tokens: 80,
                hard_limit_tokens: Some(120),
                tail_guard: 0,
                min_groups: MIN_RECENT_TURN_GROUPS,
            }
        );
    }

    /// 摘要实际保留的尾部必须原样进守卫参数：删组不许吃掉那次摘要的产物。
    #[test]
    fn tail_guard_comes_from_probe() {
        let plan = plan_compaction(&probe(budget(100, 80, 120), 150, 150, 6));
        assert_eq!(
            plan.actions[1],
            ContextAction::TrimTurnGroupsTo {
                target_tokens: 80,
                hard_limit_tokens: Some(120),
                tail_guard: 6,
                min_groups: MIN_RECENT_TURN_GROUPS,
            }
        );
    }

    /// 「同输入 → 同动作序列」：S2 搬家的等价性基准。
    #[test]
    fn same_input_same_action_sequence() {
        let inputs = [
            probe(budget(100, 80, 120), 150, 150, 0),
            probe(budget(100, 100, 120), 150, 90, 0),
            probe(budget(100, 80, 120), 50, 50, 0),
            probe(None, 150, 150, 4),
        ];
        for input in inputs {
            assert_eq!(plan_compaction(&input), plan_compaction(&input));
        }
    }

    #[test]
    fn hard_limit_fallback_requires_no_group_removal() {
        let b = budget(100, 80, 120);
        assert_eq!(
            plan_hard_limit_fallback(est(130), b, 0, 32_768),
            Some(SqueezeAllToolBodies {
                max_chars: 32_768,
                keep_groups: 0
            })
        );
        // 删过组：不再动最近组的正文。
        assert_eq!(plan_hard_limit_fallback(est(130), b, 1, 32_768), None);
        // 已在硬上限内：无需兜底。
        assert_eq!(plan_hard_limit_fallback(est(100), b, 0, 32_768), None);
        // 未配置窗口：字符预算路径不走这一档。
        assert_eq!(plan_hard_limit_fallback(est(130), None, 0, 32_768), None);
    }

    #[test]
    fn exhausted_warning_only_above_hard_limit() {
        let b = budget(100, 80, 120);
        assert_eq!(
            exhausted_warning(est(130), b),
            Some(ExhaustedWarning {
                after_tokens: 130,
                max_input_tokens: 120,
                context_window_tokens: 120,
            })
        );
        assert_eq!(exhausted_warning(est(120), b), None);
        assert_eq!(exhausted_warning(est(999), None), None);
    }
}
