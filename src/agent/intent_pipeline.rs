//! 一期意图识别管线骨架。
//!
//! 目标：提供统一 `IntentDecision` 契约，并先复用现有 `intent_router` 规则逻辑，
//! 为后续接入 L2 分类器（LLM / embedding / 专用分类模型）预留稳定入口。

use crate::agent::intent_router::{
    ExecuteIntentThresholds, IntentAssessment, IntentKind, IntentRoute,
    route_user_task_with_thresholds,
};

/// L0 上下文输入（一期先占位，后续逐步注入会话与工具轨迹特征）。
#[derive(Debug, Clone)]
pub struct IntentContext {
    /// 近期用户消息（最近优先），用于后续多轮意图增强。
    pub recent_user_messages: Vec<String>,
    /// 最近是否处于澄清流程中。
    pub in_clarification_flow: bool,
    /// 一期阈值策略（来自配置或默认值）。
    pub thresholds: ExecuteIntentThresholds,
    /// L2 分类置信度阈值；低于该值不覆盖 L1。
    pub l2_min_confidence: f32,
}

impl Default for IntentContext {
    fn default() -> Self {
        Self {
            recent_user_messages: Vec::new(),
            in_clarification_flow: false,
            thresholds: ExecuteIntentThresholds::default(),
            l2_min_confidence: 0.7,
        }
    }
}

/// L2 分类输出（可由 LLM/embedding/专用模型实现）。
#[derive(Debug, Clone, PartialEq)]
pub struct L2IntentCandidate {
    pub kind: IntentKind,
    pub primary_intent: String,
    pub secondary_intents: Vec<String>,
    pub confidence: f32,
    pub need_clarification: bool,
    pub abstain: bool,
}

/// L1/L2 合并元数据（用于观测与回归）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentMergeMeta {
    pub l1_kind: IntentKind,
    pub l1_confidence: f32,
    pub l2_present: bool,
    pub l2_applied: bool,
    pub l2_confidence: Option<f32>,
    pub override_reason: Option<String>,
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
    assess_and_route_with_l2(task, _ctx, classify_with_l2_stub(task, _ctx)).0
}

/// 合并 L1/L2 结果并返回观测元数据。
pub fn assess_and_route_with_l2(
    task: &str,
    ctx: &IntentContext,
    l2_candidate: Option<L2IntentCandidate>,
) -> (IntentDecision, IntentMergeMeta) {
    let l1_assessment = route_user_task_with_thresholds(task, ctx.thresholds);
    let l1_kind = l1_assessment.kind;
    let l1_confidence = l1_assessment.confidence;
    let mut decision = map_assessment_to_decision(task, l1_assessment);
    let mut meta = IntentMergeMeta {
        l1_kind,
        l1_confidence,
        l2_present: l2_candidate.is_some(),
        l2_applied: false,
        l2_confidence: l2_candidate.as_ref().map(|x| x.confidence),
        override_reason: None,
    };
    if let Some(l2) = l2_candidate {
        if l2.confidence >= ctx.l2_min_confidence {
            decision.kind = l2.kind;
            decision.primary_intent = l2.primary_intent;
            decision.secondary_intents = l2.secondary_intents;
            decision.confidence = l2.confidence;
            decision.need_clarification = l2.need_clarification;
            decision.abstain = l2.abstain;
            meta.l2_applied = true;
            meta.override_reason = Some("l2_confidence_above_threshold".to_string());
        } else {
            meta.override_reason = Some("l2_confidence_below_threshold".to_string());
        }
    }
    (decision, meta)
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
        "当前目录",
        "有哪些",
        "有什么",
        "有没有",
        "有无",
        "在不在",
        "是否有",
        "是否存在",
        "列出",
        "查看",
        "清单",
        "文件列表",
        "源文件",
        "源码",
        "list",
        "show files",
    ]) {
        return "execute.read_inspect";
    }
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
        "pytest",
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
        "分析",
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
    if has_any(&["跑", "执行"]) && has_any(&["test", "pytest", "cargo", "构建", "编译"]) {
        push_if_absent(&mut intents, "execute.run_test_build");
    }
    if has_any(&["定位", "排查", "分析"]) && has_any(&["报错", "异常", "panic", "error", "bug"])
    {
        push_if_absent(&mut intents, "execute.debug_diagnose");
    }
    if has_any(&["改", "修改", "重构", "实现"]) && has_any(&["函数", "文件", "模块", "代码"])
    {
        push_if_absent(&mut intents, "execute.code_change");
    }
    if has_any(&["更新", "补充", "完善", "编写"])
        && has_any(&["readme", "文档", "docs", "注释", ".md"])
    {
        push_if_absent(&mut intents, "execute.docs_ops");
    }
    if has_any(&[
        "当前目录",
        "有哪些",
        "有什么",
        "有没有",
        "有无",
        "在不在",
        "是否有",
        "是否存在",
        "列出",
        "查看",
        "清单",
        "文件列表",
        "源文件",
        "源码",
        "list",
        "show files",
    ]) {
        push_if_absent(&mut intents, "execute.read_inspect");
    }

    intents.retain(|it| it != primary_intent);
    if has_any(&["先", "再", "然后", "并且"]) && intents.is_empty() && kind == IntentKind::Execute
    {
        intents.push("execute.code_change".to_string());
        intents.retain(|it| it != primary_intent);
    }
    intents
}

fn classify_with_l2_stub(_task: &str, _ctx: &IntentContext) -> Option<L2IntentCandidate> {
    None
}

#[cfg(test)]
mod tests {
    use super::{
        IntentAction, IntentContext, L2IntentCandidate, assess_and_route, assess_and_route_with_l2,
    };
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

    #[test]
    fn readonly_listing_maps_to_read_inspect() {
        let decision = assess_and_route("当前目录下有哪些源文件", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.read_inspect");
        assert!(matches!(decision.action, IntentAction::Execute));
    }

    #[test]
    fn current_dir_has_what_maps_to_read_inspect() {
        let decision = assess_and_route("当前目录下有什么", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.read_inspect");
    }

    #[test]
    fn has_hpcg_source_code_maps_to_read_inspect() {
        let decision = assess_and_route("有hpcg的源码吗？", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.read_inspect");
    }

    #[test]
    fn run_test_maps_to_run_test_build() {
        let decision = assess_and_route("帮我跑一下 pytest", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.run_test_build");
    }

    #[test]
    fn debug_phrase_maps_to_debug_diagnose() {
        let decision = assess_and_route("这个异常请先分析定位", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.debug_diagnose");
    }

    #[test]
    fn docs_update_maps_to_docs_ops() {
        let decision = assess_and_route("请完善 docs 里的安装说明", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.docs_ops");
    }

    #[test]
    fn git_and_docs_extracts_secondary_intent() {
        let decision = assess_and_route("先更新 README 再提交并开 PR", &IntentContext::default());
        assert_eq!(decision.kind, IntentKind::Execute);
        assert_eq!(decision.primary_intent, "execute.git_ops");
        assert!(
            decision
                .secondary_intents
                .contains(&"execute.docs_ops".to_string())
        );
    }

    #[test]
    fn l2_stub_does_not_override_l1_result() {
        let decision = assess_and_route("当前目录下有哪些源文件", &IntentContext::default());
        assert_eq!(decision.primary_intent, "execute.read_inspect");
        assert!(decision.confidence > 0.0);
    }

    #[test]
    fn l2_high_confidence_overrides_l1() {
        let l2 = L2IntentCandidate {
            kind: IntentKind::Execute,
            primary_intent: "execute.docs_ops".to_string(),
            secondary_intents: vec!["execute.read_inspect".to_string()],
            confidence: 0.91,
            need_clarification: false,
            abstain: false,
        };
        let (decision, meta) = assess_and_route_with_l2(
            "当前目录下有哪些源文件",
            &IntentContext::default(),
            Some(l2),
        );
        assert_eq!(decision.primary_intent, "execute.docs_ops");
        assert!(meta.l2_applied);
    }
}
