use crate::agent_turn::decision_engine::traits::{DecisionFactor, FactorId, FactorScore};
use crate::agent_turn::decision_engine::types::FactorContext;
use crabmate_types::{Message, message_content_as_str};

/// 成本因子：基于任务规模和上下文大小评估 staged 是否划算。
///
/// 小任务（task 短、messages 少）→ 低分，freeform 更经济
/// 大任务（task 长、messages 多）→ 高分，staged 收益更高
#[derive(Debug, Default)]
pub struct CostFactor;

impl CostFactor {
    /// 任务描述中粗略估算的 token 数（每中文字符≈2 token，其他≈1）。
    fn task_rough_tokens(task: &str) -> usize {
        let (cj, other) = task.chars().fold((0usize, 0usize), |(cj, other), c| {
            if c.len_utf8() > 1 {
                (cj + 1, other)
            } else {
                (cj, other + 1)
            }
        });
        cj * 2 + other
    }

    /// 消息文本总长度。
    fn messages_total_chars(messages: &[Message]) -> usize {
        messages
            .iter()
            .filter_map(|m| message_content_as_str(&m.content))
            .map(|s| s.len())
            .sum()
    }

    fn cost_score(task: &str, messages: &[Message]) -> f32 {
        let task_tokens = Self::task_rough_tokens(task);
        let msg_chars = Self::messages_total_chars(messages);
        let total = (task_tokens + msg_chars / 4).min(2000) as f32;
        (total / 2000.0).clamp(0.0, 1.0)
    }
}

impl DecisionFactor for CostFactor {
    fn id(&self) -> FactorId {
        FactorId::Cost
    }

    fn evaluate(&self, ctx: &FactorContext) -> FactorScore {
        let raw = Self::cost_score(ctx.task, ctx.messages);
        let detail = format!(
            "task_tok={} msgs_chars={}",
            Self::task_rough_tokens(ctx.task),
            Self::messages_total_chars(ctx.messages),
        );
        FactorScore::new(self.id(), raw, self.default_weight(), detail)
    }

    fn default_weight(&self) -> f32 {
        0.10
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::{IntentAction, IntentDecision};
    use crate::intent_router::IntentKind;
    use crabmate_types::MessageContent;

    fn make_decision() -> IntentDecision {
        IntentDecision {
            kind: IntentKind::Execute,
            primary_intent: "execute.code_change".to_string(),
            secondary_intents: vec![],
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action: IntentAction::Execute,
            multi_intent: None,
        }
    }

    fn msg(text: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: Some(MessageContent::Text(text.to_string())),
            tool_calls: None,
            reasoning_content: None,
            reasoning_details: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn test_ctx(task: &'static str, msgs: Vec<Message>) -> FactorContext<'static> {
        let decision = Box::leak(Box::new(make_decision()));
        let msgs = Box::leak(Box::new(msgs));
        FactorContext {
            decision,
            task,
            messages: msgs,
            cfg: None,
            workspace_file_count: None,
        }
    }

    #[test]
    fn empty_task_returns_low_score() {
        let ctx = test_ctx("", vec![]);
        let score = CostFactor.evaluate(&ctx);
        assert!(score.raw_score < 0.1);
    }

    #[test]
    fn simple_task_returns_low_score() {
        let ctx = test_ctx("cargo build", vec![]);
        let score = CostFactor.evaluate(&ctx);
        assert!(score.raw_score < 0.3);
    }

    #[test]
    fn long_task_with_context_returns_high_score() {
        let long = "a".repeat(1500);
        let ctx = test_ctx(Box::leak(long.into_boxed_str()), vec![]);
        let score = CostFactor.evaluate(&ctx);
        assert!(score.raw_score > 0.5);
    }

    #[test]
    fn many_messages_increase_score() {
        let many_msgs: Vec<_> = (0..10).map(|i| msg(&format!("message {}", i))).collect();
        let ctx = test_ctx("fix bug", many_msgs);
        let score_empty = CostFactor.evaluate(&test_ctx("fix bug", vec![]));
        let score_full = CostFactor.evaluate(&ctx);
        assert!(score_full.raw_score > score_empty.raw_score);
    }

    #[test]
    fn task_rough_tokens_cjk() {
        let tokens = CostFactor::task_rough_tokens("重构 auth 模块");
        assert!(tokens > 0);
    }
}
