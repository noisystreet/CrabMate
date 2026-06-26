//! 一期意图识别管线骨架。
//!
//! 目标：提供统一 `IntentDecision` 契约，并先复用现有 `intent_router` 规则逻辑，
//! 为后续接入 L2 分类器（LLM / embedding / 专用分类模型）预留稳定入口。

use crate::intent_l0::{self, IntentL0Snapshot};
use crate::intent_router::{
    ExecuteIntentThresholds, IntentAssessment, IntentKind, IntentRoute,
    is_explicit_execute_confirmation, route_user_task_with_thresholds,
};

/// 意图管线上下文；`recent_user_messages` 为**当前** user 条**之前**的近期 user 正文（**新在前**）；
/// 澄清续接时与 `intent_l0::effective_intent_routing_text` 拼成路由文本。
#[derive(Debug, Clone)]
pub struct IntentContext {
    pub recent_user_messages: Vec<String>,
    pub in_clarification_flow: bool,
    pub thresholds: ExecuteIntentThresholds,
    pub l2_min_confidence: f32,
    /// 当前 user 前消息尾部是否存在失败 `role: tool`；见 `intent_l0::messages_have_recent_tool_failure`。
    pub has_recent_tool_failure: bool,
    /// 为 false 时跳过 L0 对 L1 的**保守提级/抬档**（仍保留续接合并与 L0 观测）。
    pub l0_routing_boost_enabled: bool,
}

impl Default for IntentContext {
    fn default() -> Self {
        Self {
            recent_user_messages: Vec::new(),
            in_clarification_flow: false,
            thresholds: ExecuteIntentThresholds::default(),
            l2_min_confidence: 0.7,
            has_recent_tool_failure: false,
            l0_routing_boost_enabled: true,
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
    /// 澄清流程下是否将前序 user 与当前短句拼成**路由**文本供 L1/L2 使用。
    pub used_merged_continuation: bool,
    /// 对合并/当前路由文本的 L0 可观测特征（含 `has_recent_tool_failure` 等）。
    pub l0: IntentL0Snapshot,
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

/// 意图管线入口：L0 多轮路由合并与特征快照、L1、可选 L2（stub 无 L2）与 L3 动作映射。
pub fn assess_and_route(task: &str, ctx: &IntentContext) -> IntentDecision {
    let (routing, used_merge, l0) = prepare_intent_routing(task, ctx);
    assess_and_route_with_l2_inner(
        &routing,
        task,
        &l0,
        used_merge,
        ctx,
        classify_with_l2_stub(task, ctx),
    )
    .0
}

/// 对当前 `task` 与 `ctx` 做 L0 续接合并与 L0 快照，供 L1/L2 与观测共用（含 `has_recent_tool_failure`）。
pub fn prepare_intent_routing(
    current_task: &str,
    ctx: &IntentContext,
) -> (String, bool, IntentL0Snapshot) {
    let (routing, used_merge) = intent_l0::effective_intent_routing_text(
        current_task,
        ctx.in_clarification_flow,
        &ctx.recent_user_messages,
    );
    let l0 = intent_l0::l0_snapshot_merged(&routing, ctx.has_recent_tool_failure);
    (routing, used_merge, l0)
}

/// 合并 L1/L2 结果并返回观测元数据。
pub fn assess_and_route_with_l2(
    current_task: &str,
    ctx: &IntentContext,
    l2_candidate: Option<L2IntentCandidate>,
) -> (IntentDecision, IntentMergeMeta) {
    let (routing, used_merge, l0) = prepare_intent_routing(current_task, ctx);
    assess_and_route_with_l2_inner(&routing, current_task, &l0, used_merge, ctx, l2_candidate)
}

/// `routing` 为 L1 输入；`primary_task` 为细粒度 heuristics，通常取当前用户句原文。
fn assess_and_route_with_l2_inner(
    routing: &str,
    primary_task: &str,
    l0: &IntentL0Snapshot,
    used_merged_continuation: bool,
    ctx: &IntentContext,
    l2_candidate: Option<L2IntentCandidate>,
) -> (IntentDecision, IntentMergeMeta) {
    let mut l1_assessment = route_user_task_with_thresholds(routing, ctx.thresholds);
    if ctx.l0_routing_boost_enabled {
        maybe_boost_execute_from_l0(&mut l1_assessment, ctx.thresholds, l0);
    }
    let normalized = routing.trim().to_lowercase();
    if ctx.in_clarification_flow && is_explicit_execute_confirmation(&normalized) {
        l1_assessment = IntentAssessment {
            kind: IntentKind::Execute,
            confidence: 0.96,
            route: IntentRoute::Execute,
        };
    }
    let l1_kind = l1_assessment.kind;
    let l1_confidence = l1_assessment.confidence;
    let l1_route = l1_assessment.route.clone();
    let mut decision = map_assessment_to_decision(primary_task, l1_assessment);
    let mut meta = IntentMergeMeta {
        l1_kind,
        l1_confidence,
        l2_present: l2_candidate.is_some(),
        l2_applied: false,
        l2_confidence: l2_candidate.as_ref().map(|x| x.confidence),
        override_reason: None,
        used_merged_continuation,
        l0: *l0,
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
            refresh_decision_action_after_l2_override(&mut decision, l1_kind, &l1_route);
        } else {
            meta.override_reason = Some("l2_confidence_below_threshold".to_string());
        }
    }
    (decision, meta)
}

/// L2 覆盖 `kind` 后，与 `DirectReply` 模板、执行阈值路由对齐（避免仍沿用 L1 的回复文案或动作）。
fn refresh_decision_action_after_l2_override(
    decision: &mut IntentDecision,
    l1_kind: IntentKind,
    l1_route: &IntentRoute,
) {
    use crate::intent_router::{
        ambiguous_ask_message, greeting_reply_message, qa_direct_reply_for_primary,
    };
    match decision.kind {
        IntentKind::Qa => {
            decision.action =
                IntentAction::DirectReply(qa_direct_reply_for_primary(&decision.primary_intent));
        }
        IntentKind::Greeting => {
            decision.action = IntentAction::DirectReply(greeting_reply_message().to_string());
            decision.need_clarification = false;
            decision.abstain = false;
        }
        IntentKind::Ambiguous => {
            decision.action = IntentAction::ClarifyThenExecute(ambiguous_ask_message().to_string());
        }
        IntentKind::Execute => {
            if l1_kind == IntentKind::Execute {
                match l1_route {
                    IntentRoute::Execute => {
                        decision.action = IntentAction::Execute;
                        decision.need_clarification = false;
                        decision.abstain = false;
                    }
                    IntentRoute::ConfirmThenExecute(s) => {
                        decision.action = IntentAction::ConfirmThenExecute(s.clone());
                        decision.need_clarification = true;
                        decision.abstain = false;
                    }
                    IntentRoute::AskThenExecute(s) => {
                        decision.action = IntentAction::ClarifyThenExecute(s.clone());
                        decision.need_clarification = true;
                    }
                    IntentRoute::DirectReply(_) => {
                        decision.action = IntentAction::Execute;
                        decision.need_clarification = false;
                        decision.abstain = false;
                    }
                }
            } else {
                decision.action = IntentAction::Execute;
                decision.need_clarification = false;
                decision.abstain = false;
            }
        }
    }
}

/// L0 为「路径/错误/构建/近期 tool 失败」等时，将偏 Ambiguous 的**合并路由**提级为可执行，减少无意义追问。
fn maybe_boost_execute_from_l0(
    a: &mut IntentAssessment,
    thresholds: ExecuteIntentThresholds,
    l0: &IntentL0Snapshot,
) {
    if a.kind == IntentKind::Ambiguous
        && l0.has_file_path_like
        && (l0.has_error_signal
            || l0.has_command_cargo
            || l0.has_git_keyword
            || l0.has_recent_tool_failure)
    {
        let conf = 0.62_f32.max(a.confidence);
        *a = IntentAssessment {
            kind: IntentKind::Execute,
            confidence: conf,
            route: if conf >= thresholds.high {
                IntentRoute::Execute
            } else {
                IntentRoute::ConfirmThenExecute(crate::intent_router::EXECUTE_CONFIRM.to_string())
            },
        };
        return;
    }
    if a.kind == IntentKind::Execute
        && matches!(&a.route, IntentRoute::ConfirmThenExecute(_))
        && l0.has_file_path_like
        && (l0.has_error_signal || l0.has_command_cargo)
        && a.confidence + 0.12 >= thresholds.high
    {
        let c = (a.confidence + 0.12).min(1.0);
        a.confidence = c;
        a.route = IntentRoute::Execute;
    }
}

fn map_assessment_to_decision(task: &str, assessment: IntentAssessment) -> IntentDecision {
    let primary_intent = map_primary_intent(task, assessment.kind).to_string();
    let secondary_intents = map_secondary_intents(task, assessment.kind, &primary_intent);
    match &assessment.route {
        IntentRoute::Execute => IntentDecision {
            kind: assessment.kind,
            primary_intent,
            secondary_intents,
            confidence: assessment.confidence,
            abstain: false,
            need_clarification: false,
            action: IntentAction::Execute,
        },
        IntentRoute::DirectReply(reply) => {
            let body = if assessment.kind == IntentKind::Qa {
                if reply.starts_with("收到，我先不执行") {
                    reply.clone()
                } else {
                    crate::intent_router::qa_direct_reply_for_primary(&primary_intent)
                }
            } else {
                reply.clone()
            };
            IntentDecision {
                kind: assessment.kind,
                primary_intent,
                secondary_intents,
                confidence: assessment.confidence,
                abstain: false,
                need_clarification: false,
                action: IntentAction::DirectReply(body),
            }
        }
        IntentRoute::AskThenExecute(reply) => IntentDecision {
            kind: assessment.kind,
            primary_intent,
            secondary_intents,
            confidence: assessment.confidence,
            abstain: assessment.kind == IntentKind::Ambiguous,
            need_clarification: true,
            action: IntentAction::ClarifyThenExecute(reply.clone()),
        },
        IntentRoute::ConfirmThenExecute(reply) => IntentDecision {
            kind: assessment.kind,
            primary_intent,
            secondary_intents,
            confidence: assessment.confidence,
            abstain: false,
            need_clarification: true,
            action: IntentAction::ConfirmThenExecute(reply.clone()),
        },
    }
}

fn map_primary_intent(task: &str, kind: IntentKind) -> &'static str {
    match kind {
        IntentKind::Greeting => "meta.greeting",
        IntentKind::Qa => map_qa_primary_intent(task),
        IntentKind::Execute => map_execute_primary_intent(task),
        IntentKind::Ambiguous => "unknown",
    }
}

fn map_qa_primary_intent(task: &str) -> &'static str {
    let n = task.to_lowercase();
    let raw = task.trim();
    if (raw.contains("我刚才")
        || raw.contains("我上一条")
        || raw.contains("上一句")
        || raw.contains("上一条"))
        && (raw.contains("什么")
            || raw.contains("问了")
            || raw.contains("说的")
            || raw.contains("提问")
            || raw.contains("问题")
            || raw.contains("原文"))
    {
        return "qa.meta";
    }
    if raw.contains("你会") && raw.contains('吗') && !raw.contains("帮我") {
        return "qa.meta";
    }
    if n.contains("介绍") && (n.contains('你') || n.contains("自己")) {
        return "qa.meta";
    }
    if n.contains("你是谁")
        || n.contains("你叫什么")
        || n.contains("自我介绍一下")
        || n.contains("技能")
        || n.contains("你能做什么")
        || n.contains("你能帮我")
        || n.contains("你有哪些")
        || n.contains("能力范围")
    {
        return "qa.meta";
    }
    "qa.explain"
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

fn push_secondary_keyword_hit(
    intents: &mut Vec<String>,
    normalized: &str,
    keywords: &[&str],
    id: &str,
) {
    if keywords.iter().any(|k| normalized.contains(k)) {
        push_secondary_unique(intents, id);
    }
}

fn push_secondary_unique(buf: &mut Vec<String>, v: &str) {
    if !buf.iter().any(|x| x == v) {
        buf.push(v.to_string());
    }
}

fn push_secondary_dual_keyword_hit(
    intents: &mut Vec<String>,
    normalized: &str,
    a: &[&str],
    b: &[&str],
    id: &str,
) {
    if a.iter().any(|k| normalized.contains(k)) && b.iter().any(|k| normalized.contains(k)) {
        push_secondary_unique(intents, id);
    }
}

fn map_secondary_intents(task: &str, kind: IntentKind, primary_intent: &str) -> Vec<String> {
    if kind != IntentKind::Execute {
        return Vec::new();
    }
    let normalized = task.to_lowercase();
    let mut intents = Vec::new();

    push_secondary_keyword_hit(
        &mut intents,
        &normalized,
        &[
            "commit",
            "提交",
            "pr",
            "pull request",
            "cherry-pick",
            "rebase",
            "merge",
            "branch",
        ],
        "execute.git_ops",
    );
    push_secondary_keyword_hit(
        &mut intents,
        &normalized,
        &[
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
        ],
        "execute.run_test_build",
    );
    push_secondary_keyword_hit(
        &mut intents,
        &normalized,
        &[
            "报错", "error", "panic", "异常", "失败", "定位", "排查", "调试", "诊断", "修复", "bug",
        ],
        "execute.debug_diagnose",
    );
    push_secondary_keyword_hit(
        &mut intents,
        &normalized,
        &["文档", "readme", "docs/", "注释", "说明", "md"],
        "execute.docs_ops",
    );
    push_secondary_keyword_hit(
        &mut intents,
        &normalized,
        &[
            "改", "修改", "实现", "重构", "新增", "删除", ".rs", ".ts", ".tsx", ".py",
        ],
        "execute.code_change",
    );

    push_secondary_dual_keyword_hit(
        &mut intents,
        &normalized,
        &["跑", "执行"],
        &["test", "pytest", "cargo", "构建", "编译"],
        "execute.run_test_build",
    );
    push_secondary_dual_keyword_hit(
        &mut intents,
        &normalized,
        &["定位", "排查", "分析"],
        &["报错", "异常", "panic", "error", "bug"],
        "execute.debug_diagnose",
    );
    push_secondary_dual_keyword_hit(
        &mut intents,
        &normalized,
        &["改", "修改", "重构", "实现"],
        &["函数", "文件", "模块", "代码"],
        "execute.code_change",
    );
    push_secondary_dual_keyword_hit(
        &mut intents,
        &normalized,
        &["更新", "补充", "完善", "编写"],
        &["readme", "文档", "docs", "注释", ".md"],
        "execute.docs_ops",
    );

    push_secondary_keyword_hit(
        &mut intents,
        &normalized,
        &[
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
        ],
        "execute.read_inspect",
    );

    intents.retain(|it| it != primary_intent);
    if intents.is_empty()
        && (normalized.contains("先")
            || normalized.contains("再")
            || normalized.contains("然后")
            || normalized.contains("并且"))
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
    use super::{IntentContext, L2IntentCandidate, assess_and_route_with_l2};
    use crate::intent_router::IntentKind;

    /// 细粒度断言见 `fixtures/intent_regression.jsonl`（`cargo test golden_intent_regression`）。

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

    /// L2 置信度低于 `ctx.l2_min_confidence` 时不得覆盖 L1（与 `classify_intent_l2_with_llm` fail-open 对齐）。
    #[test]
    fn l2_below_threshold_does_not_override_l1() {
        let ctx = IntentContext {
            l2_min_confidence: 0.75,
            ..Default::default()
        };
        let l2 = L2IntentCandidate {
            kind: IntentKind::Execute,
            primary_intent: "execute.code_change".to_string(),
            secondary_intents: Vec::new(),
            confidence: 0.74,
            need_clarification: false,
            abstain: false,
        };
        let (decision, meta) = assess_and_route_with_l2("当前目录下有哪些源文件", &ctx, Some(l2));
        assert!(!meta.l2_applied);
        assert_eq!(
            meta.override_reason.as_deref(),
            Some("l2_confidence_below_threshold")
        );
        assert_ne!(decision.primary_intent, "execute.code_change");
    }
}
