//! 分层模式：意图门控 **`ProceedExecute`** 之后的**显式路由**（纯函数，无 IO）。
//!
//! 将原 `hierarchy::run_hierarchical_agent` 内嵌的 `skip_manager_for_discourse` 布尔链收束为
//! [`HierarchicalPostIntentRoute`]，便于 `tracing`、回放与单测对齐。

use crate::intent_pipeline::{IntentAction, IntentDecision};
use crate::intent_router::{
    IntentKind, intent_reply_delegates_to_main_model, qa_readonly_style_primary,
};

/// 意图门控已 **`ProceedExecute`** 时，下一执行面（与 [`super::hierarchy::HierarchicalRunPhase`] 语义对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchicalPostIntentRoute {
    /// 话语型 / 澄清确认 / 只读 QA：跳 Manager，走 **`run_agent_outer_loop`**。
    DiscourseFallbackOuter(HierarchicalDiscourseFallbackReason),
    /// Router → Manager → Operator → `run_hierarchical`。
    RouterManagerRunner,
}

/// 走 **`DiscourseFallbackOuter`** 的判定原因（用于日志与回放；不含机密）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchicalDiscourseFallbackReason {
    /// [`intent_reply_delegates_to_main_model`]：寒暄、`qa.meta*`、`qa.explain` 等改走主模型。
    DelegatesToMainModel,
    /// `ClarifyThenExecute`：注入追问提示后主循环。
    ClarifyThenExecute,
    /// `ConfirmThenExecute`：注入确认提示后主循环。
    ConfirmThenExecute,
    /// `Qa` + `qa.readonly*` + `DirectReply`：只读门控后主循环。
    QaReadonlyDirectReply,
}

impl HierarchicalDiscourseFallbackReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DelegatesToMainModel => "delegates_to_main_model",
            Self::ClarifyThenExecute => "clarify_then_execute",
            Self::ConfirmThenExecute => "confirm_then_execute",
            Self::QaReadonlyDirectReply => "qa_readonly_direct_reply",
        }
    }
}

impl HierarchicalPostIntentRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DiscourseFallbackOuter(_) => "discourse_fallback_outer",
            Self::RouterManagerRunner => "router_manager_runner",
        }
    }
}

/// 将 L2 优先管线产出的 **`IntentDecision`** 解析为分层内下一主路径（**仅**在 `ProceedExecute` 分支调用）。
pub fn resolve_hierarchical_post_intent_route(
    assessment: &IntentDecision,
) -> HierarchicalPostIntentRoute {
    if intent_reply_delegates_to_main_model(assessment.kind, &assessment.primary_intent) {
        return HierarchicalPostIntentRoute::DiscourseFallbackOuter(
            HierarchicalDiscourseFallbackReason::DelegatesToMainModel,
        );
    }
    if let IntentAction::ClarifyThenExecute(_) = &assessment.action {
        return HierarchicalPostIntentRoute::DiscourseFallbackOuter(
            HierarchicalDiscourseFallbackReason::ClarifyThenExecute,
        );
    }
    if let IntentAction::ConfirmThenExecute(_) = &assessment.action {
        return HierarchicalPostIntentRoute::DiscourseFallbackOuter(
            HierarchicalDiscourseFallbackReason::ConfirmThenExecute,
        );
    }
    if assessment.kind == IntentKind::Qa
        && qa_readonly_style_primary(&assessment.primary_intent)
        && matches!(&assessment.action, IntentAction::DirectReply(_))
    {
        return HierarchicalPostIntentRoute::DiscourseFallbackOuter(
            HierarchicalDiscourseFallbackReason::QaReadonlyDirectReply,
        );
    }
    HierarchicalPostIntentRoute::RouterManagerRunner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::IntentDecision;

    fn decision(kind: IntentKind, primary: &str, action: IntentAction) -> IntentDecision {
        IntentDecision {
            kind,
            primary_intent: primary.to_string(),
            secondary_intents: Vec::new(),
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action,
        }
    }

    #[test]
    fn readonly_qa_direct_reply_is_discourse() {
        let d = decision(
            IntentKind::Qa,
            "qa.readonly.foo",
            IntentAction::DirectReply("x".into()),
        );
        assert_eq!(
            resolve_hierarchical_post_intent_route(&d),
            HierarchicalPostIntentRoute::DiscourseFallbackOuter(
                HierarchicalDiscourseFallbackReason::QaReadonlyDirectReply
            )
        );
    }

    #[test]
    fn execute_read_inspect_is_router_manager() {
        let d = decision(
            IntentKind::Execute,
            "execute.read_inspect",
            IntentAction::Execute,
        );
        assert_eq!(
            resolve_hierarchical_post_intent_route(&d),
            HierarchicalPostIntentRoute::RouterManagerRunner
        );
    }

    #[test]
    fn greeting_delegates_to_outer() {
        let d = decision(
            IntentKind::Greeting,
            "meta.greeting",
            IntentAction::DirectReply("".into()),
        );
        assert_eq!(
            resolve_hierarchical_post_intent_route(&d),
            HierarchicalPostIntentRoute::DiscourseFallbackOuter(
                HierarchicalDiscourseFallbackReason::DelegatesToMainModel
            )
        );
    }

    #[test]
    fn clarify_branch_is_discourse() {
        let d = decision(
            IntentKind::Ambiguous,
            "unknown",
            IntentAction::ClarifyThenExecute("?".into()),
        );
        assert_eq!(
            resolve_hierarchical_post_intent_route(&d),
            HierarchicalPostIntentRoute::DiscourseFallbackOuter(
                HierarchicalDiscourseFallbackReason::ClarifyThenExecute
            )
        );
    }
}
