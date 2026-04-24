//! 一期意图识别管线骨架。
//!
//! 目标：提供统一 `IntentDecision` 契约，并先复用现有 `intent_router` 规则逻辑，
//! 为后续接入 L2 分类器（LLM / embedding / 专用分类模型）预留稳定入口。

use crate::agent::intent_router::{
    ExecuteIntentThresholds, IntentAssessment, IntentKind, IntentRoute,
    route_user_task_with_thresholds,
};

/// L0 上下文输入（一期先占位，后续逐步注入会话与工具轨迹特征）。
#[derive(Debug, Clone, Default)]
pub struct IntentContext {
    /// 近期用户消息（最近优先），用于后续多轮意图增强。
    pub recent_user_messages: Vec<String>,
    /// 最近是否处于澄清流程中。
    pub in_clarification_flow: bool,
    /// 一期阈值策略（来自配置或默认值）。
    pub thresholds: ExecuteIntentThresholds,
}

/// L3 决策动作：执行、直接回复、先澄清或先确认。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentAction {
    Execute,
    DirectReply(String),
    ClarifyThenExecute(String),
    ConfirmThenExecute(String),
}

/// 统一意图决策结构（供 agent_turn/hierarchy 等上层消费）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentDecision {
    /// 兼容旧分类：Greeting / Qa / Execute / Ambiguous。
    pub kind: IntentKind,
    /// 细粒度主意图（一期先占位映射，后续由 L2 分类器输出）。
    pub primary_intent: String,
    /// 次意图（一期默认空）。
    pub secondary_intents: Vec<String>,
    /// 置信度，区间 [0.0, 1.0]。
    pub confidence: f32,
    /// 是否拒识（abstain）。
    pub abstain: bool,
    /// 是否需要澄清。
    pub need_clarification: bool,
    /// 动作决策。
    pub action: IntentAction,
}

/// 意图管线入口（一期）。
///
/// 当前实现：
/// - L0: 接收上下文（暂不参与计算）
/// - L1/L2: 复用 `intent_router` 规则与阈值
/// - L3: 统一动作映射
pub fn assess_and_route(task: &str, _ctx: &IntentContext) -> IntentDecision {
    let assessment = route_user_task_with_thresholds(task, _ctx.thresholds);
    map_assessment_to_decision(task, assessment)
}

fn map_assessment_to_decision(task: &str, assessment: IntentAssessment) -> IntentDecision {
    let primary_intent = map_primary_intent(task, assessment.kind).to_string();
    let secondary_intents = map_secondary_intents(task, assessment.kind, &primary_intent);
    match assessment.route {
        IntentRoute::Execute => IntentDecision {
            kind: assessment.kind,
            primary_intent,
            secondary_intents,
            confidence: assessment.confidence,
            abstain: false,
            need_clarification: false,
            action: IntentAction::Execute,
        },
        IntentRoute::DirectReply(reply) => IntentDecision {
            kind: assessment.kind,
            primary_intent,
            secondary_intents,
            confidence: assessment.confidence,
            abstain: false,
            need_clarification: false,
            action: IntentAction::DirectReply(reply),
        },
        IntentRoute::AskThenExecute(reply) => IntentDecision {
            kind: assessment.kind,
            primary_intent,
            secondary_intents,
            confidence: assessment.confidence,
            abstain: assessment.kind == IntentKind::Ambiguous,
            need_clarification: true,
            action: IntentAction::ClarifyThenExecute(reply),
        },
        IntentRoute::ConfirmThenExecute(reply) => IntentDecision {
            kind: assessment.kind,
            primary_intent,
            secondary_intents,
            confidence: assessment.confidence,
            abstain: false,
            need_clarification: true,
            action: IntentAction::ConfirmThenExecute(reply),
        },
    }
}

fn map_primary_intent(task: &str, kind: IntentKind) -> &'static str {
    match kind {
        IntentKind::Greeting => "meta.greeting",
        IntentKind::Qa => "qa.explain",
        IntentKind::Execute => map_execute_primary_intent(task),
        IntentKind::Ambiguous => "unknown",
    }
}

fn map_execute_primary_intent(task: &str) -> &'static str {
    let normalized = task.to_lowercase();
    let has_any = |keywords: &[&str]| keywords.iter().any(|k| normalized.contains(k));

    if has_any(&[
        "commit",
        "提交",
        "pr",
        "pull request",
        "cherry-pick",
        "rebase",
        "merge",
        "branch",
    ]) {
        return "execute.git_ops";
    }
    if has_any(&[
        "测试",
        "test",
        "cargo test",
        "cargo build",
        "构建",
        "编译",
        "build",
        "run",
        "运行",
        "clippy",
        "fmt",
    ]) {
        return "execute.run_test_build";
    }
    if has_any(&[
        "报错", "error", "panic", "异常", "失败", "定位", "排查", "调试", "诊断", "修复", "bug",
    ]) {
        return "execute.debug_diagnose";
    }
    if has_any(&["文档", "readme", "docs/", "注释", "说明", "md"]) {
        return "execute.docs_ops";
    }
    "execute.code_change"
}

fn map_secondary_intents(task: &str, kind: IntentKind, primary_intent: &str) -> Vec<String> {
    if kind != IntentKind::Execute {
        return Vec::new();
    }
    let normalized = task.to_lowercase();
    let has_any = |keywords: &[&str]| keywords.iter().any(|k| normalized.contains(k));
    let mut intents = Vec::new();
    let push_if_absent = |buf: &mut Vec<String>, v: &str| {
        if !buf.iter().any(|x| x == v) {
            buf.push(v.to_string());
        }
    };

    if has_any(&[
        "commit",
        "提交",
        "pr",
        "pull request",
        "cherry-pick",
        "rebase",
        "merge",
        "branch",
    ]) {
        push_if_absent(&mut intents, "execute.git_ops");
    }
    if has_any(&[
        "测试",
        "test",
        "cargo test",
        "cargo build",
        "构建",
        "编译",
        "build",
        "run",
        "运行",
        "clippy",
        "fmt",
    ]) {
        push_if_absent(&mut intents, "execute.run_test_build");
    }
    if has_any(&[
        "报错", "error", "panic", "异常", "失败", "定位", "排查", "调试", "诊断", "修复", "bug",
    ]) {
        push_if_absent(&mut intents, "execute.debug_diagnose");
    }
    if has_any(&["文档", "readme", "docs/", "注释", "说明", "md"]) {
        push_if_absent(&mut intents, "execute.docs_ops");
    }
    if has_any(&[
        "改", "修改", "实现", "重构", "新增", "删除", ".rs", ".ts", ".tsx", ".py",
    ]) {
        push_if_absent(&mut intents, "execute.code_change");
    }

    intents.retain(|it| it != primary_intent);
    intents
}

#[cfg(test)]
mod tests {
    use super::{IntentAction, IntentContext, assess_and_route};
    use crate::agent::intent_router::IntentKind;

    #[test]
    fn execute_routes_to_execute_action() {
        let decision = assess_and_route("帮我修复这个报错", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Execute);
        assert!(matches!(decision.action, IntentAction::Execute));
        assert_eq!(decision.primary_intent, "execute.debug_diagnose");
    }

    #[test]
    fn qa_routes_to_direct_reply_action() {
        let decision = assess_and_route("这个错误是什么意思？", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Qa);
        assert!(matches!(decision.action, IntentAction::DirectReply(_)));
        assert!(!decision.need_clarification);
    }

    #[test]
    fn ambiguous_routes_to_clarification_and_abstain() {
        let decision = assess_and_route("帮我看看", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Ambiguous);
        assert!(matches!(
            decision.action,
            IntentAction::ClarifyThenExecute(_)
        ));
        assert!(decision.need_clarification);
        assert!(decision.abstain);
        assert_eq!(decision.primary_intent, "unknown");
    }

    #[test]
    fn build_like_request_maps_to_run_test_build() {
        let decision = assess_and_route(
            "请帮我运行 cargo test 并修复失败项",
            &IntentContext::default(),
        );
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.run_test_build");
    }

    #[test]
    fn git_like_request_maps_to_git_ops() {
        let decision = assess_and_route("把这些改动提交并开一个 PR", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.git_ops");
    }

    #[test]
    fn mixed_request_extracts_secondary_intents() {
        let decision = assess_and_route(
            "先跑 cargo test 修复失败，再把改动提交并开 PR",
            &IntentContext::default(),
        );
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.git_ops");
        assert!(
            decision
                .secondary_intents
                .contains(&"execute.run_test_build".to_string())
        );
        assert!(
            decision
                .secondary_intents
                .contains(&"execute.debug_diagnose".to_string())
        );
        assert!(
            !decision
                .secondary_intents
                .contains(&"execute.git_ops".to_string())
        );
    }
}
