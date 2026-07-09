use crate::agent_turn::decision_engine::traits::{DecisionFactor, FactorId, FactorScore};
use crate::agent_turn::decision_engine::types::FactorContext;

/// 复杂度因子：基于 token 数和文件引用估算任务复杂度。
///
/// - token 数：`task.chars().count()` 线性映射到 [0, 1]（50-500 字符范围）
/// - 文件引用：检测任务中文件路径模式（如 `*.rs`、`*.py`），计数映射到 [0, 1]
/// - 综合得分 = token_score × 0.5 + file_score × 0.5
#[derive(Debug, Default)]
pub struct ComplexityFactor;

impl ComplexityFactor {
    const MIN_CHARS: usize = 50;
    const MAX_CHARS: usize = 500;
    const MAX_FILES: usize = 5;

    fn token_score(task: &str) -> f32 {
        let chars = task.chars().count();
        if chars <= Self::MIN_CHARS {
            0.0
        } else if chars >= Self::MAX_CHARS {
            1.0
        } else {
            (chars - Self::MIN_CHARS) as f32 / (Self::MAX_CHARS - Self::MIN_CHARS) as f32
        }
    }

    fn file_ref_score(task: &str) -> f32 {
        let count = count_file_references(task);
        if count == 0 {
            0.0
        } else {
            (count.min(Self::MAX_FILES) as f32) / Self::MAX_FILES as f32
        }
    }
}

impl DecisionFactor for ComplexityFactor {
    fn id(&self) -> FactorId {
        FactorId::Complexity
    }

    fn evaluate(&self, ctx: &FactorContext) -> FactorScore {
        let token = Self::token_score(ctx.task);
        let file = Self::file_ref_score(ctx.task);
        let mut raw = (token * 0.5 + file * 0.5).clamp(0.0, 1.0);

        // 多意图加分（Phase 2.5，仅 L2 来源）
        if let Some(ref mi) = ctx.decision.multi_intent {
            raw += 0.3 * (mi.item_count.min(5) as f32 / 5.0);
        }

        raw = raw.clamp(0.0, 1.0);

        let detail = if ctx.decision.multi_intent.is_some() {
            format!(
                "token={:.2} file={:.2} multi_intent=N={}",
                token,
                file,
                ctx.decision
                    .multi_intent
                    .as_ref()
                    .map_or(0, |mi| mi.item_count)
            )
        } else {
            format!("token={:.2} file={:.2}", token, file)
        };
        FactorScore::new(self.id(), raw, self.default_weight(), detail)
    }

    fn default_weight(&self) -> f32 {
        0.25
    }
}

/// 检测任务文本中文件路径模式的数量。
fn count_file_references(text: &str) -> usize {
    let mut count = 0usize;
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'.' && i + 1 < len && bytes[i + 1].is_ascii_alphanumeric() {
            let mut j = i + 2;
            while j < len && bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            let ext_len = j - i - 1;
            if (1..=6).contains(&ext_len) {
                count += 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::{IntentAction, IntentDecision, IntentRelation, MultiIntentInfo};
    use crate::intent_router::IntentKind;

    fn make_ctx_with_multi_intent(
        subtasks: Vec<&str>,
        relation: IntentRelation,
    ) -> FactorContext<'static> {
        let decision = IntentDecision {
            kind: IntentKind::Execute,
            primary_intent: "execute.code_change".to_string(),
            secondary_intents: vec![],
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action: IntentAction::Execute,
            multi_intent: Some(MultiIntentInfo {
                item_count: subtasks.len(),
                relation,
            }),
        };
        let task = Box::leak(Box::new(
            subtasks
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        ));
        let decision = Box::leak(Box::new(decision));
        FactorContext {
            decision,
            task,
            messages: &[],
            cfg: None,
            workspace_file_count: None,
        }
    }

    #[test]
    fn token_score_empty_task_returns_zero() {
        assert_eq!(ComplexityFactor::token_score(""), 0.0);
    }

    #[test]
    fn token_score_short_task_returns_zero() {
        assert_eq!(ComplexityFactor::token_score("cargo build"), 0.0);
    }

    #[test]
    fn token_score_long_task_returns_one() {
        let long = "x".repeat(501);
        assert_eq!(ComplexityFactor::token_score(&long), 1.0);
    }

    #[test]
    fn file_ref_score_no_files_returns_zero() {
        assert_eq!(ComplexityFactor::file_ref_score("hello world"), 0.0);
    }

    #[test]
    fn file_ref_score_detects_single_file() {
        let score = ComplexityFactor::file_ref_score("edit src/main.rs");
        assert!(score > 0.0);
    }

    #[test]
    fn file_ref_score_detects_multiple_files() {
        let score =
            ComplexityFactor::file_ref_score("refactor src/main.rs, src/lib.rs, and tests/test.rs");
        assert!(score >= 0.6);
    }

    #[test]
    fn file_ref_score_caps_at_max() {
        let many = (0..10)
            .map(|i| format!("file{}.rs", i))
            .collect::<Vec<_>>()
            .join(" ");
        let score = ComplexityFactor::file_ref_score(&many);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn multi_intent_parallel_adds_bonus() {
        let ctx = make_ctx_with_multi_intent(
            vec!["重构 auth 模块", "添加单元测试"],
            IntentRelation::Parallel,
        );
        let score = ComplexityFactor.evaluate(&ctx);
        // 2 items → bonus = 0.3 * 2/5 = 0.12
        assert!(score.raw_score > 0.0);
        assert!(score.detail.contains("multi_intent"));
    }

    #[test]
    fn multi_intent_sequential_adds_bonus() {
        let ctx = make_ctx_with_multi_intent(
            vec!["修复登录 bug", "优化数据库查询", "更新文档"],
            IntentRelation::Sequential,
        );
        let score = ComplexityFactor.evaluate(&ctx);
        // 3 items → bonus = 0.3 * 3/5 = 0.18
        assert!(score.raw_score > 0.0);
        assert!(score.detail.contains("multi_intent"));
    }

    #[test]
    fn no_multi_intent_no_bonus() {
        let factor = ComplexityFactor;
        let ctx = FactorContext {
            decision: &IntentDecision {
                kind: IntentKind::Execute,
                primary_intent: "execute.code_change".to_string(),
                secondary_intents: vec![],
                confidence: 0.9,
                abstain: false,
                need_clarification: false,
                action: IntentAction::Execute,
                multi_intent: None,
            },
            task: "cargo build",
            messages: &[],
            cfg: None,
            workspace_file_count: None,
        };
        let score = factor.evaluate(&ctx);
        assert!(!score.detail.contains("multi_intent"));
    }
}
