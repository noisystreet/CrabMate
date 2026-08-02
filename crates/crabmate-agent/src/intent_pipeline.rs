//! 意图识别管线（fail-open / 确认续接 / L0 观测；无 L2）。
//!
//! L2 额外 chat 与相关配置键已退役（R3）。金样见 `fixtures/intent_regression.jsonl`。

use crate::intent_l0::{self, IntentL0Snapshot};
use crate::intent_router::{IntentKind, is_explicit_execute_confirmation};

/// 意图管线上下文；`recent_user_messages` 为**当前** user 条**之前**的近期 user 正文（**新在前**）。
#[derive(Debug, Clone, Default)]
pub struct IntentContext {
    pub recent_user_messages: Vec<String>,
    pub in_clarification_flow: bool,
    /// 当前 user 前消息尾部是否存在失败 `role: tool`。
    pub has_recent_tool_failure: bool,
}

/// 意图决策元数据（观测与回归）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentMergeMeta {
    pub baseline_kind: IntentKind,
    pub baseline_confidence: f32,
    pub override_reason: Option<String>,
    /// 澄清流程下是否将前序 user 与当前短句拼成路由文本。
    pub used_merged_continuation: bool,
    pub l0: IntentL0Snapshot,
    /// 确认续接 / 失败续跑为 false；普通 fail-open 为 true（门控不再据此收窄工具）。
    pub fail_open_conservative: bool,
}

/// 意图动作（R3 起生产与金样仅 `Execute`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentAction {
    Execute,
}

/// 统一意图决策结构。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentDecision {
    pub kind: IntentKind,
    pub primary_intent: String,
    pub secondary_intents: Vec<String>,
    pub confidence: f32,
    pub abstain: bool,
    pub need_clarification: bool,
    pub action: IntentAction,
}

/// 意图管线入口：fail-open 为 Execute（供金样 / 单测）。
pub fn assess_and_route(task: &str, ctx: &IntentContext) -> IntentDecision {
    assess_and_route_with_meta(task, ctx).0
}

/// 评估意图并返回观测元数据。
pub fn assess_and_route_with_meta(
    current_task: &str,
    ctx: &IntentContext,
) -> (IntentDecision, IntentMergeMeta) {
    let (routing, used_merge, l0) = prepare_intent_routing(current_task, ctx);
    assess_fail_open_inner(&routing, current_task, &l0, used_merge, ctx)
}

/// 对当前 `task` 与 `ctx` 做续接合并与 L0 快照。
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
    let meta = IntentMergeMeta {
        baseline_kind: decision.kind,
        baseline_confidence: decision.confidence,
        override_reason: Some(format!("fail_open;baseline={baseline_reason}")),
        used_merged_continuation,
        l0: *l0,
        fail_open_conservative: baseline_conservative,
    };
    log::info!(
        target: "crabmate_intent",
        "intent_classification primary={:?} action={:?} baseline_conf={:.3} fail_open_conservative={} override={} l0={:?}",
        decision.primary_intent,
        decision.action,
        meta.baseline_confidence,
        meta.fail_open_conservative,
        meta.override_reason.as_deref().unwrap_or("none"),
        l0,
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

    #[test]
    fn fail_open_is_execute_and_conservative() {
        let (decision, meta) = assess_and_route_with_meta(
            "帮我编写一个简单c++程序，然后使用cmake编译执行",
            &IntentContext::default(),
        );
        assert!(meta.fail_open_conservative);
        assert!(
            meta.override_reason
                .as_deref()
                .is_some_and(|s| s.starts_with("fail_open;baseline=")),
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
