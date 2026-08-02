//! 意图识别管线。
//!
//! L2 额外 chat 已退役（R2）：本模块仅 **fail-open 为 Execute**、确认续接与失败续跑启发式，
//! 以及 L0 续接合并 / 可观测特征。`IntentMergeMeta` 中 `l2_*` / `suggested_mode` 字段保留至 R3 清扫，
//! 现行路径恒为未调用 L2。

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
    /// 历史字段：L2 阈值已无决策作用（R2 退役调用）；R3 再删。
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

/// 意图决策元数据（用于观测与回归）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentMergeMeta {
    /// 基线决策（fail-open / 确认续接）；历史字段名 `l1_*`。
    pub l1_kind: IntentKind,
    /// 基线置信度；历史字段名 `l1_confidence`。
    pub l1_confidence: f32,
    /// 历史字段：R2 起恒为 `false`（无 L2 调用）。
    pub l2_present: bool,
    /// 历史字段：R2 起恒为 `false`。
    pub l2_applied: bool,
    /// 历史字段：R2 起恒为 `None`。
    pub l2_confidence: Option<f32>,
    /// 历史字段：R2 起为退役原因（如 `retired_r2`）。
    pub l2_unavailable_reason: Option<String>,
    /// 历史字段：R2 起恒为 `None`（原 L2 `suggested_mode`）。
    pub suggested_mode: Option<String>,
    pub override_reason: Option<String>,
    /// 澄清流程下是否将前序 user 与当前短句拼成**路由**文本。
    pub used_merged_continuation: bool,
    /// 对合并/当前路由文本的 L0 可观测特征（含 `has_recent_tool_failure` 等）。
    pub l0: IntentL0Snapshot,
    /// 历史：曾表示 fail-open 时门控侧应收窄只读；R2 起门控不再据此收窄，仅观测。
    pub fail_open_conservative: bool,
}

impl IntentMergeMeta {
    /// 是否应对本轮 Apply **ReviewReadonly**（历史 API；R2 起门控不消费）。
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

/// 多意图信息（历史：仅 L2 填充；R2 起恒空）。
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

/// 统一意图决策结构（供 agent_turn 等上层消费）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentDecision {
    /// 兼容旧分类：Greeting / Qa / Execute / Ambiguous。
    pub kind: IntentKind,
    /// 细粒度主意图标签（fail-open 粗分；不驱动路由）。
    pub primary_intent: String,
    /// 次意图（默认空）。
    pub secondary_intents: Vec<String>,
    /// 置信度，区间 [0.0, 1.0]。
    pub confidence: f32,
    /// 是否拒识（abstain）。
    pub abstain: bool,
    /// 是否需要澄清。
    pub need_clarification: bool,
    /// 动作决策。
    pub action: IntentAction,
    /// 多意图解析结果（R2 起恒 `None`）。
    pub multi_intent: Option<MultiIntentInfo>,
}

/// 意图管线入口：fail-open 为 Execute（供金样 / 单测）。
pub fn assess_and_route(task: &str, ctx: &IntentContext) -> IntentDecision {
    assess_and_route_with_meta(task, ctx).0
}

/// 评估意图并返回观测元数据（无 L2）。
pub fn assess_and_route_with_meta(
    current_task: &str,
    ctx: &IntentContext,
) -> (IntentDecision, IntentMergeMeta) {
    let (routing, used_merge, l0) = prepare_intent_routing(current_task, ctx);
    assess_fail_open_inner(&routing, current_task, &l0, used_merge, ctx)
}

/// 对当前 `task` 与 `ctx` 做续接合并与 L0 快照（含 `has_recent_tool_failure`）。
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

fn assess_fail_open_inner(
    routing: &str,
    primary_task: &str,
    l0: &IntentL0Snapshot,
    used_merged_continuation: bool,
    ctx: &IntentContext,
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
    let meta = IntentMergeMeta {
        l1_kind,
        l1_confidence,
        l2_present: false,
        l2_applied: false,
        l2_confidence: None,
        l2_unavailable_reason: Some("retired_r2".to_string()),
        suggested_mode: None,
        override_reason: Some(format!("fail_open_l2_retired;baseline={baseline_reason}")),
        used_merged_continuation,
        l0: *l0,
        fail_open_conservative: baseline_conservative,
    };
    log::info!(
        target: "crabmate_intent",
        "intent_classification primary={:?} action={:?} baseline_conf={:.3} fail_open_conservative={} override={} l0_kind={:?} subtasks={}",
        decision.primary_intent,
        decision.action,
        l1_confidence,
        meta.fail_open_conservative,
        meta.override_reason.as_deref().unwrap_or("none"),
        l0,
        decision.multi_intent.as_ref().map_or(0, |mi| mi.item_count),
    );
    (decision, meta)
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

#[cfg(test)]
mod tests {
    use super::{IntentContext, assess_and_route_with_meta};

    /// 细粒度断言见 `fixtures/intent_regression.jsonl`（`cargo test golden_intent_regression`）。

    #[test]
    fn fail_open_marks_retired_l2_and_conservative() {
        let (decision, meta) = assess_and_route_with_meta(
            "帮我编写一个简单c++程序，然后使用cmake编译执行",
            &IntentContext::default(),
        );
        assert!(!meta.l2_present);
        assert!(!meta.l2_applied);
        assert!(meta.fail_open_conservative);
        assert!(meta.suggested_mode.is_none());
        assert_eq!(meta.l2_unavailable_reason.as_deref(), Some("retired_r2"));
        assert!(
            meta.override_reason
                .as_deref()
                .is_some_and(|s| s.starts_with("fail_open_l2_retired")),
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
        let (decision, meta) = assess_and_route_with_meta("直接开始执行", &ctx);
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
}
