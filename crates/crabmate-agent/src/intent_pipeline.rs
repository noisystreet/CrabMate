//! 意图识别管线。
//!
//! L2 仅产出 **`suggested_mode`**（UI 提示，不自动改用户会话 mode）。
//! L2 不可用或低于观测阈值时 **fail-open 为 Execute**（进主模型），不再走已移除的 L1 关键词路由。
//! L0 仅负责续接合并与可观测特征。

use crate::intent_l0::{self, IntentL0Snapshot};
use crate::intent_router::{ExecuteIntentThresholds, IntentKind, is_explicit_execute_confirmation};

/// 意图管线上下文；`recent_user_messages` 为**当前** user 条**之前**的近期 user 正文（**新在前**）；
/// 澄清续接时与 `intent_l0::effective_intent_routing_text` 拼成路由文本。
#[derive(Debug, Clone)]
pub struct IntentContext {
    pub recent_user_messages: Vec<String>,
    pub in_clarification_flow: bool,
    /// 历史字段：L1 阈值已不再驱动决策，保留以免破坏调用方构造。
    pub thresholds: ExecuteIntentThresholds,
    pub l2_min_confidence: f32,
    /// 当前 user 前消息尾部是否存在失败 `role: tool`；见 `intent_l0::messages_have_recent_tool_failure`。
    pub has_recent_tool_failure: bool,
    /// 历史字段：L0→L1 提级已移除；保留以免破坏调用方构造。
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

/// L2 分类输出（可由 LLM 实现）：仅建议会话模式，**不**自动改写用户 `session_mode`。
#[derive(Debug, Clone, PartialEq)]
pub struct L2IntentCandidate {
    /// `ask` / `plan` / `act`
    pub suggested_mode: String,
    pub confidence: f32,
}

/// L2 分类尝试结果；`candidate=None` 时 `unavailable_reason` 说明为何 fail-open。
#[derive(Debug, Clone, PartialEq)]
pub struct L2IntentAttempt {
    pub candidate: Option<L2IntentCandidate>,
    pub unavailable_reason: Option<String>,
}

impl L2IntentAttempt {
    pub fn from_candidate(candidate: Option<L2IntentCandidate>) -> Self {
        Self {
            candidate,
            unavailable_reason: None,
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            candidate: None,
            unavailable_reason: Some(reason.into()),
        }
    }
}

/// 意图决策元数据（用于观测与回归）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentMergeMeta {
    /// 基线决策（fail-open / 确认续接）；历史字段名 `l1_*`。
    pub l1_kind: IntentKind,
    /// 基线置信度；历史字段名 `l1_confidence`。
    pub l1_confidence: f32,
    pub l2_present: bool,
    pub l2_applied: bool,
    pub l2_confidence: Option<f32>,
    pub l2_unavailable_reason: Option<String>,
    /// L2 建议的会话模式（UI 提示；不自动落盘/不改请求 mode）。
    pub suggested_mode: Option<String>,
    pub override_reason: Option<String>,
    /// 澄清流程下是否将前序 user 与当前短句拼成**路由**文本供 L2 使用。
    pub used_merged_continuation: bool,
    /// 对合并/当前路由文本的 L0 可观测特征（含 `has_recent_tool_failure` 等）。
    pub l0: IntentL0Snapshot,
    /// fail-open 进主循环时采用保守工具策略（门控侧应收窄为只读）；确认续接/失败续跑为 false。
    pub fail_open_conservative: bool,
}

impl IntentMergeMeta {
    /// 是否应对本轮 Apply **ReviewReadonly**（L2 缺失/低置信 Execute fail-open）。
    #[must_use]
    pub fn should_apply_conservative_tool_policy(&self) -> bool {
        self.fail_open_conservative
    }
}

/// L3 决策动作：执行、直接回复、先澄清或先确认。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentAction {
    Execute,
    DirectReply(String),
    ClarifyThenExecute(String),
    ConfirmThenExecute(String),
}

/// 多意图信息（仅 L2 填充）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiIntentInfo {
    pub item_count: usize,
    pub relation: IntentRelation,
}

/// 多意图之间的关系类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentRelation {
    Parallel,
    Sequential,
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
    /// 多意图解析结果（仅 L2 填充）。
    pub multi_intent: Option<MultiIntentInfo>,
}

/// 意图管线入口：无 L2 stub 时 fail-open 为 Execute（供金样 / 单测）。
pub fn assess_and_route(task: &str, ctx: &IntentContext) -> IntentDecision {
    let (routing, used_merge, l0) = prepare_intent_routing(task, ctx);
    assess_and_route_with_l2_inner(
        &routing,
        task,
        &l0,
        used_merge,
        ctx,
        L2IntentAttempt::from_candidate(classify_with_l2_stub(task, ctx)),
    )
    .0
}

/// 对当前 `task` 与 `ctx` 做续接合并与 L0 快照，供 L2 与观测共用（含 `has_recent_tool_failure`）。
pub fn prepare_intent_routing(
    current_task: &str,
    ctx: &IntentContext,
) -> (String, bool, IntentL0Snapshot) {
    let (routing, used_merge) = intent_l0::effective_intent_routing_text(
        current_task,
        ctx.in_clarification_flow,
        &ctx.recent_user_messages,
        ctx.has_recent_tool_failure,
    );
    let l0 = intent_l0::l0_snapshot_merged(&routing, ctx.has_recent_tool_failure);
    (routing, used_merge, l0)
}

/// 评估意图并返回观测元数据；有足够置信的 L2 结果时采纳，否则 fail-open。
pub fn assess_and_route_with_l2(
    current_task: &str,
    ctx: &IntentContext,
    l2_candidate: Option<L2IntentCandidate>,
) -> (IntentDecision, IntentMergeMeta) {
    assess_and_route_with_l2_attempt(
        current_task,
        ctx,
        L2IntentAttempt::from_candidate(l2_candidate),
    )
}

/// 评估意图并携带 L2 不可用原因；有足够置信的 L2 结果时采纳，否则 fail-open。
pub fn assess_and_route_with_l2_attempt(
    current_task: &str,
    ctx: &IntentContext,
    l2_attempt: L2IntentAttempt,
) -> (IntentDecision, IntentMergeMeta) {
    let (routing, used_merge, l0) = prepare_intent_routing(current_task, ctx);
    assess_and_route_with_l2_inner(&routing, current_task, &l0, used_merge, ctx, l2_attempt)
}

/// `routing` 为 L2 合并上下文；`primary_task` 为当前用户句原文。
fn assess_and_route_with_l2_inner(
    routing: &str,
    primary_task: &str,
    l0: &IntentL0Snapshot,
    used_merged_continuation: bool,
    ctx: &IntentContext,
    l2_attempt: L2IntentAttempt,
) -> (IntentDecision, IntentMergeMeta) {
    let mut decision = fail_open_execute_decision(primary_task);
    let mut baseline_reason = "fail_open_execute";
    let mut baseline_conservative = true;
    let normalized = routing.trim().to_lowercase();
    if ctx.in_clarification_flow && is_explicit_execute_confirmation(&normalized) {
        decision = fail_open_execute_decision_with_confidence(primary_task, 0.96);
        baseline_reason = "explicit_execute_confirmation";
        baseline_conservative = false;
    } else if used_merged_continuation
        && ctx.has_recent_tool_failure
        && intent_l0::is_resume_after_failure_utterance(primary_task.trim())
    {
        decision = fail_open_execute_decision_with_confidence(primary_task, 0.94);
        baseline_reason = "resume_after_tool_failure";
        baseline_conservative = false;
    }
    let l1_kind = decision.kind;
    let l1_confidence = decision.confidence;
    let l2_confidence = l2_attempt.candidate.as_ref().map(|x| x.confidence);
    let mut meta = IntentMergeMeta {
        l1_kind,
        l1_confidence,
        l2_present: l2_attempt.candidate.is_some(),
        l2_applied: false,
        l2_confidence,
        l2_unavailable_reason: l2_attempt.unavailable_reason.clone(),
        suggested_mode: None,
        override_reason: None,
        used_merged_continuation,
        l0: *l0,
        fail_open_conservative: false,
    };
    if let Some(l2) = l2_attempt.candidate {
        let l2_confidence = l2.confidence;
        meta.suggested_mode = Some(l2.suggested_mode.clone());
        if l2_confidence >= ctx.l2_min_confidence {
            decision = map_l2_candidate_to_decision(l2);
            meta.l2_applied = true;
            meta.override_reason = Some("l2_suggested_mode".to_string());
            meta.fail_open_conservative = false;
        } else {
            // 低置信：fail-open 进主循环，工具策略保守（只读）；仍保留 suggested_mode 供 UI。
            meta.override_reason = Some(format!(
                "l2_below_threshold_fail_open;baseline={baseline_reason}"
            ));
            meta.fail_open_conservative = baseline_conservative;
        }
    } else {
        meta.override_reason = Some(match meta.l2_unavailable_reason.as_deref() {
            Some("disabled_by_config") => {
                format!("fail_open_l2_disabled;baseline={baseline_reason}")
            }
            Some("skipped_gate_off") | Some("skipped_session_mode_readonly") => {
                format!("fail_open_l2_skipped;baseline={baseline_reason}")
            }
            Some(_) => format!("fail_open_l2_unavailable;baseline={baseline_reason}"),
            None => format!("fail_open_no_l2;baseline={baseline_reason}"),
        });
        meta.fail_open_conservative = baseline_conservative;
    }
    log::info!(
        target: "crabmate_intent",
        "intent_classification primary={:?} action={:?} baseline_conf={:.3} l2_conf={:?} l2_applied={} suggested_mode={:?} fail_open_conservative={} override={} l0_kind={:?} subtasks={}",
        decision.primary_intent,
        decision.action,
        l1_confidence,
        l2_confidence,
        meta.l2_applied,
        meta.suggested_mode,
        meta.fail_open_conservative,
        meta.override_reason.as_deref().unwrap_or("none"),
        l0,
        decision.multi_intent.as_ref().map_or(0, |mi| mi.item_count),
    );
    (decision, meta)
}

fn map_l2_candidate_to_decision(l2: L2IntentCandidate) -> IntentDecision {
    let mode = normalize_suggested_mode(&l2.suggested_mode).unwrap_or("act");
    IntentDecision {
        kind: IntentKind::Execute,
        primary_intent: format!("suggested_mode.{mode}"),
        secondary_intents: Vec::new(),
        confidence: l2.confidence,
        abstain: false,
        need_clarification: false,
        action: IntentAction::Execute,
        multi_intent: None,
    }
}

/// 规范化 L2 `suggested_mode`；非法则 `None`。
#[must_use]
pub fn normalize_suggested_mode(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "ask" => Some("ask"),
        "plan" => Some("plan"),
        "act" => Some("act"),
        _ => None,
    }
}

fn fail_open_execute_decision(task: &str) -> IntentDecision {
    fail_open_execute_decision_with_confidence(task, 0.5)
}

fn fail_open_execute_decision_with_confidence(task: &str, confidence: f32) -> IntentDecision {
    IntentDecision {
        kind: IntentKind::Execute,
        primary_intent: map_execute_primary_intent(task).to_string(),
        secondary_intents: Vec::new(),
        confidence,
        abstain: false,
        need_clarification: false,
        action: IntentAction::Execute,
        multi_intent: None,
    }
}

/// fail-open 观测用的粗粒度 execute 子类标签（不驱动路由）。
fn map_execute_primary_intent(task: &str) -> &'static str {
    let normalized = task.to_lowercase();
    let has_any = |keywords: &[&str]| keywords.iter().any(|k| normalized.contains(k));

    if has_any(&["当前目录", "文件列表", "源文件", "list", "show files"])
        || ((has_any(&[
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
        ]) && has_any(&["目录", "文件", "源码", "源文件", "仓库", "项目"]))
            || (normalized.contains('有')
                && (normalized.contains("源码") || normalized.contains("文件"))
                && normalized.contains('吗')))
    {
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
        "报错", "错误", "error", "panic", "异常", "失败", "定位", "排查", "调试", "诊断", "修复",
        "bug", "分析",
    ]) {
        return "execute.debug_diagnose";
    }
    if has_any(&["文档", "readme", "docs/", "注释", "说明", "md"]) {
        return "execute.docs_ops";
    }
    "execute.code_change"
}

fn classify_with_l2_stub(_task: &str, _ctx: &IntentContext) -> Option<L2IntentCandidate> {
    None
}

#[cfg(test)]
mod tests {
    use super::{
        IntentContext, L2IntentAttempt, L2IntentCandidate, assess_and_route_with_l2,
        assess_and_route_with_l2_attempt,
    };

    /// 细粒度断言见 `fixtures/intent_regression.jsonl`（`cargo test golden_intent_regression`）。

    #[test]
    fn l2_high_confidence_suggested_mode_applied() {
        let l2 = L2IntentCandidate {
            suggested_mode: "ask".to_string(),
            confidence: 0.91,
        };
        let (decision, meta) = assess_and_route_with_l2(
            "当前目录下有哪些源文件",
            &IntentContext::default(),
            Some(l2),
        );
        assert_eq!(decision.primary_intent, "suggested_mode.ask");
        assert_eq!(meta.suggested_mode.as_deref(), Some("ask"));
        assert!(meta.l2_applied);
        assert!(!meta.fail_open_conservative);
        assert!(matches!(decision.action, super::IntentAction::Execute));
    }

    #[test]
    fn l2_below_threshold_fail_opens_conservative_keeps_suggestion() {
        let ctx = IntentContext {
            l2_min_confidence: 0.75,
            ..Default::default()
        };
        let l2 = L2IntentCandidate {
            suggested_mode: "act".to_string(),
            confidence: 0.74,
        };
        let (decision, meta) = assess_and_route_with_l2("当前目录下有哪些源文件", &ctx, Some(l2));
        assert!(!meta.l2_applied);
        assert!(meta.fail_open_conservative);
        assert_eq!(meta.suggested_mode.as_deref(), Some("act"));
        assert!(
            meta.override_reason
                .as_deref()
                .is_some_and(|s| s.starts_with("l2_below_threshold_fail_open")),
            "override={:?}",
            meta.override_reason
        );
        assert_eq!(decision.primary_intent, "execute.read_inspect");
        assert!(matches!(decision.action, super::IntentAction::Execute));
    }

    #[test]
    fn l2_unavailable_reason_is_preserved_for_fail_open() {
        let (decision, meta) = assess_and_route_with_l2_attempt(
            "帮我编写一个简单c++程序，然后使用cmake编译执行",
            &IntentContext::default(),
            L2IntentAttempt::unavailable("api_key_missing"),
        );
        assert!(!meta.l2_present);
        assert!(!meta.l2_applied);
        assert!(meta.fail_open_conservative);
        assert!(meta.suggested_mode.is_none());
        assert_eq!(
            meta.l2_unavailable_reason.as_deref(),
            Some("api_key_missing")
        );
        assert!(
            meta.override_reason
                .as_deref()
                .is_some_and(|s| s.starts_with("fail_open_l2_unavailable")),
            "override={:?}",
            meta.override_reason
        );
        assert!(matches!(decision.action, super::IntentAction::Execute));
    }

    #[test]
    fn explicit_confirm_fail_open_is_not_conservative() {
        let ctx = IntentContext {
            in_clarification_flow: true,
            ..Default::default()
        };
        let (decision, meta) = assess_and_route_with_l2_attempt(
            "直接开始执行",
            &ctx,
            L2IntentAttempt::unavailable("api_key_missing"),
        );
        assert!(!meta.fail_open_conservative);
        assert!(matches!(decision.action, super::IntentAction::Execute));
        assert!(
            meta.override_reason
                .as_deref()
                .is_some_and(|s| s.contains("baseline=explicit_execute_confirmation")),
            "override={:?}",
            meta.override_reason
        );
    }

    #[test]
    fn normalize_suggested_mode_accepts_ask_plan_act() {
        assert_eq!(super::normalize_suggested_mode("ASK"), Some("ask"));
        assert_eq!(super::normalize_suggested_mode(" plan "), Some("plan"));
        assert_eq!(super::normalize_suggested_mode("act"), Some("act"));
        assert_eq!(super::normalize_suggested_mode("nope"), None);
    }
}
