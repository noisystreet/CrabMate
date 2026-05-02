//! 意图管线 **`IntentContext`** 的**单一构建入口**（L0 尾部工具失败信号 + 近期 user 合并 + 阈值等），
//! 供 **`intent/staged_planning_gate`**（含 **`assess_staged_planning_gate`** / **`assess_staged_planning_gate_full_pipeline`**）与 **`intent/at_turn_start`** 共用，避免两处字段漂移。

use crate::agent::intent_l0;
use crate::agent::intent_pipeline::IntentContext;
use crate::agent::intent_router::ExecuteIntentThresholds;
use crate::config::AgentConfig;
use crate::types::Message;

use super::intent_user;

const RECENT_USER_FOR_MERGE: usize = 4;
const MSG_TAIL_FOR_TOOL: usize = 32;

/// 从会话切片与阈值构造 **`IntentContext`**（**不**调用 L1/L2，仅上下文装配）。
pub(crate) fn build_intent_routing_context(
    messages: &[Message],
    cfg: &AgentConfig,
    in_clarification_flow: bool,
    thresholds: ExecuteIntentThresholds,
) -> IntentContext {
    let has_recent_tool_failure =
        intent_l0::messages_have_recent_tool_failure(messages, MSG_TAIL_FOR_TOOL);
    let recent_user_messages =
        intent_user::collect_recent_user_messages(messages, RECENT_USER_FOR_MERGE);
    IntentContext {
        recent_user_messages,
        in_clarification_flow,
        thresholds,
        l2_min_confidence: cfg.intent_l2_min_confidence,
        has_recent_tool_failure,
        l0_routing_boost_enabled: cfg.intent_l0_routing_boost_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    #[test]
    fn same_inputs_same_context_fields() {
        let cfg = crate::config::load_config(None).expect("embed default config");
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
