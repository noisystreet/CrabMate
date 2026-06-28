//! 意图管线 **`IntentContext`** 的**单一构建入口**（L0 尾部工具失败信号 + 近期 user 合并 + 阈值等）。

use crabmate_config::AgentConfig;
use crabmate_types::Message;

use crate::intent_l0;
use crate::intent_pipeline::IntentContext;
use crate::intent_router::ExecuteIntentThresholds;

use super::user;

const RECENT_USER_FOR_MERGE: usize = 4;
const MSG_TAIL_FOR_TOOL: usize = 32;

/// 从会话切片与阈值构造 **`IntentContext`**（**不**调用 L2 或弃用规则层，仅上下文装配）。
pub fn build_intent_routing_context(
    messages: &[Message],
    cfg: &AgentConfig,
    in_clarification_flow: bool,
    thresholds: ExecuteIntentThresholds,
) -> IntentContext {
    let has_recent_tool_failure =
        intent_l0::messages_have_recent_tool_failure(messages, MSG_TAIL_FOR_TOOL);
    let recent_user_messages = user::collect_recent_user_messages(messages, RECENT_USER_FOR_MERGE);
    IntentContext {
        recent_user_messages,
        in_clarification_flow,
        thresholds,
        l2_min_confidence: cfg.intent_routing.intent_l2_min_confidence,
        has_recent_tool_failure,
        l0_routing_boost_enabled: cfg.intent_routing.intent_l0_routing_boost_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_types::Message;

    #[test]
    fn same_inputs_same_context_fields() {
        let cfg = crabmate_config::load_config(None).expect("embed default config");
        let messages = vec![Message::user_only("hello".to_string())];
        let th = ExecuteIntentThresholds {
            low: 0.4,
            high: 0.75,
        };
        let a = build_intent_routing_context(&messages, &cfg, false, th);
        let b = build_intent_routing_context(&messages, &cfg, false, th);
        assert_eq!(a.thresholds.low, b.thresholds.low);
        assert_eq!(a.has_recent_tool_failure, b.has_recent_tool_failure);
        assert_eq!(a.l0_routing_boost_enabled, b.l0_routing_boost_enabled);
    }
}
