//! 非分层模式下 **`resolve_non_hierarchical_main_route`** 之前的分阶段意图门控：
//! 与 [`super::at_turn_start::run_intent_l0_l1_l2_gate`] **共用**同一套 L0+L1+可选 L2 管线（见 **`assess_staged_planning_gate_full_pipeline`**），避免仅用 L1（**`assess_and_route`**）与开局门控分叉。
//!
//! 同步 **`assess_staged_planning_gate`**（仅 L1）保留供不需要异步/`RunLoopParams` 的单测与快速探测。

use crate::agent::intent_l2_classifier::classify_intent_l2_with_llm;
use crate::agent::intent_pipeline::{
    IntentAction, IntentDecision, assess_and_route_with_l2, prepare_intent_routing,
};
use crate::agent::intent_router::{ExecuteIntentThresholds, IntentKind};

#[cfg(test)]
use crate::agent::intent_pipeline::assess_and_route;
#[cfg(test)]
use crate::config::AgentConfig;
#[cfg(test)]
use crate::types::Message;

use super::at_turn_start::emit_intent_timeline_gate_only;
use super::build_intent_routing_context;
use super::intent_user;

/// 非分层路径下，是否允许进入分阶段 / 逻辑双代理编排（仅 `IntentAction::Execute` 为 true）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StagedPlanningGateOutcome {
    /// 意图管线判定为「执行任务」，可分流到 staged / logical dual。
    Allow {
        task_preview: String,
        intent_kind: IntentKind,
        primary_intent: String,
        confidence: f32,
        decision: IntentDecision,
    },
    /// 无可路由的有效 user 任务句，或管线未给出 Execute，或命中「架构/重构咨询」启发式而跳过滚动分阶段规划。
    Deny {
        reason: StagedPlanningDenyReason,
        task_preview: Option<String>,
        intent_decision: Option<IntentDecision>,
    },
}

/// 拒绝进入分阶段编排的原因（用于日志与单测；不含机密）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlanningDenyReason {
    /// `extract_effective_user_task` 为空（无 user 或全文空白）。
    EmptyEffectiveTask,
    /// 管线已跑通，但 `action != Execute`（直接回复 / 澄清 / 确认等）。
    IntentPipelineNotExecute,
    /// 管线判定为 **Execute**，但正文命中「架构/重构咨询」启发式：不进入滚动分阶段规划，改走单 Agent 外循环（避免无工具规划轮把咨询拆成大量读文件步）。
    AdvisoryExecuteBypassStaged,
}

impl StagedPlanningDenyReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::EmptyEffectiveTask => "empty_effective_task",
            Self::IntentPipelineNotExecute => "intent_pipeline_not_execute",
            Self::AdvisoryExecuteBypassStaged => "advisory_execute_bypass_staged",
        }
    }
}

impl StagedPlanningGateOutcome {
    pub(crate) fn allows_staged_planning(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }
}

fn intent_action_discriminant(action: &IntentAction) -> &'static str {
    match action {
        IntentAction::Execute => "execute",
        IntentAction::DirectReply(_) => "direct_reply",
        IntentAction::ClarifyThenExecute(_) => "clarify_then_execute",
        IntentAction::ConfirmThenExecute(_) => "confirm_then_execute",
    }
}

/// 是否因「架构/重构类咨询」跳过滚动分阶段规划（在 **`IntentAction::Execute`** 前提下）。
fn should_bypass_staged_for_advisory_execute_task(task: &str, decision: &IntentDecision) -> bool {
    if !matches!(decision.action, IntentAction::Execute) {
        return false;
    }
    let lower = task.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }

    let impl_strength = [
        "请修改",
        "请实现",
        "请添加",
        "请删除",
        "帮我改",
        "帮我写",
        "帮我删",
        "直接改",
        "直接写",
        "运行 cargo",
        "cargo test",
        "cargo build",
        "cargo fmt",
        "提交",
        "开 pr",
        "pull request",
        "cherry-pick",
        "rebase",
        "fix bug",
        "implement ",
        "add feature",
        "apply_patch",
    ];
    if impl_strength.iter().any(|k| lower.contains(k)) {
        return false;
    }

    let arch = [
        "重构",
        "架构",
        "隐式状态",
        "技术债",
        "耦合",
        "模块边界",
        "解耦",
        "分层",
        "implicit state",
        "architecture",
        "refactoring strategy",
        "refactor plan",
    ];
    let consult = [
        "哪里",
        "哪些",
        "如何",
        "怎么",
        "建议",
        "分析",
        "说明",
        "介绍",
        "严重",
        "痛点",
        "值得",
        "要不要",
        "哪些方面",
        "有何问题",
        "什么问题",
        "where ",
        "what parts",
        "which areas",
        "how should",
        "suggest",
        "recommend",
    ];

    let has_arch = arch.iter().any(|k| lower.contains(k));
    let has_consult = consult.iter().any(|k| lower.contains(k));
    (lower.contains("隐式") || has_arch) && has_consult
}

/// 在 **`IntentAction::Execute`** 且未命中咨询启发式时返回 `Ok`；否则返回对应 **Deny** 原因（含非 Execute）。
fn staged_plan_eligibility_for_intent(
    task: &str,
    decision: &IntentDecision,
) -> Result<(), StagedPlanningDenyReason> {
    if !matches!(decision.action, IntentAction::Execute) {
        return Err(StagedPlanningDenyReason::IntentPipelineNotExecute);
    }
    if should_bypass_staged_for_advisory_execute_task(task, decision) {
        return Err(StagedPlanningDenyReason::AdvisoryExecuteBypassStaged);
    }
    Ok(())
}

fn log_staged_gate_outcome(
    task: &str,
    decision: &IntentDecision,
    sse_tag: &'static str,
    eligibility: Result<(), StagedPlanningDenyReason>,
) {
    let allowed = eligibility.is_ok();
    let deny_reason = eligibility.err().map(StagedPlanningDenyReason::as_str);
    log::info!(
        target: "crabmate",
        "{sse_tag} outcome={} reason={} task_preview={} kind={:?} primary={} action_discriminant={} confidence={:.3}",
        if allowed { "allow" } else { "deny" },
        if allowed {
            "execute_intent"
        } else {
            deny_reason.unwrap_or("deny")
        },
        crate::redact::preview_chars(task, 80),
        decision.kind,
        decision.primary_intent,
        intent_action_discriminant(&decision.action),
        decision.confidence
    );
}

/// 评估本回合是否允许进入分阶段 / 逻辑双代理路径（完整 **L0+L1+可选 L2**，与非分层开局门控对齐）。
pub(crate) async fn assess_staged_planning_gate_full_pipeline(
    p: &mut crate::agent::agent_turn::params::RunLoopParams<'_>,
    sse_log_tag: &'static str,
) -> StagedPlanningGateOutcome {
    let in_clarification_flow =
        intent_user::recently_waiting_execute_confirmation(p.turn.messages());
    let task = intent_user::extract_effective_user_task(p.turn.messages(), in_clarification_flow);
    if task.trim().is_empty() {
        log::info!(
            target: "crabmate",
            "staged_plan_intent_gate outcome=deny reason=empty_effective_task"
        );
        return StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::EmptyEffectiveTask,
            task_preview: None,
            intent_decision: None,
        };
    }

    let intent_ctx = build_intent_routing_context(
        p.turn.messages(),
        p.ctx.core.cfg.as_ref(),
        in_clarification_flow,
        ExecuteIntentThresholds {
            low: p
                .ctx
                .core
                .cfg
                .intent_routing
                .intent_non_hier_execute_low_threshold,
            high: p
                .ctx
                .core
                .cfg
                .intent_routing
                .intent_non_hier_execute_high_threshold,
        },
    );
    let (routing_for_l1, _, _) = prepare_intent_routing(task.as_str(), &intent_ctx);
    let l2_candidate = if p.ctx.core.cfg.intent_routing.intent_l2_enabled {
        classify_intent_l2_with_llm(
            &routing_for_l1,
            task.as_str(),
            p.ctx.core.cfg.as_ref(),
            p.ctx.core.llm_backend,
            p.ctx.core.client,
            p.ctx.core.api_key,
        )
        .await
    } else {
        None
    };
    let (decision, merge_meta) = assess_and_route_with_l2(task.as_str(), &intent_ctx, l2_candidate);

    log::info!(
        target: "crabmate",
        "[INTENT_PIPELINE] {sse_log_tag} staged_plan_intent_gate l1_kind={:?} l1_confidence={:.2} l2_present={} l2_applied={} l2_confidence={:?} override={:?} final_kind={:?} primary={} conf={:.2} abstain={} need_clarif={} action={:?} merged_continuation={}",
        merge_meta.l1_kind,
        merge_meta.l1_confidence,
        merge_meta.l2_present,
        merge_meta.l2_applied,
        merge_meta.l2_confidence,
        merge_meta.override_reason,
        decision.kind,
        decision.primary_intent,
        decision.confidence,
        decision.abstain,
        decision.need_clarification,
        &decision.action,
        merge_meta.used_merged_continuation,
    );

    let suppress_timeline = p.turn.take_suppress_duplicate_intent_timeline_once();
    if !suppress_timeline {
        emit_intent_timeline_gate_only(p.ctx.io.out, sse_log_tag, &decision, &merge_meta).await;
    }

    let eligibility = staged_plan_eligibility_for_intent(task.as_str(), &decision);
    log_staged_gate_outcome(task.as_str(), &decision, sse_log_tag, eligibility);

    match eligibility {
        Ok(()) => StagedPlanningGateOutcome::Allow {
            task_preview: task,
            intent_kind: decision.kind,
            primary_intent: decision.primary_intent.clone(),
            confidence: decision.confidence,
            decision,
        },
        Err(reason) => StagedPlanningGateOutcome::Deny {
            reason,
            task_preview: Some(task),
            intent_decision: Some(decision),
        },
    }
}

/// 同步门控（仅 **L1**，无 L2）；用于单测与无需 LLM 的探测。
#[cfg(test)]
pub(crate) fn assess_staged_planning_gate(
    messages: &[Message],
    cfg: &AgentConfig,
) -> StagedPlanningGateOutcome {
    let in_clarification_flow = intent_user::recently_waiting_execute_confirmation(messages);
    let task = intent_user::extract_effective_user_task(messages, in_clarification_flow);
    if task.trim().is_empty() {
        log::info!(
            target: "crabmate",
            "staged_plan_intent_gate outcome=deny reason=empty_effective_task"
        );
        return StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::EmptyEffectiveTask,
            task_preview: None,
            intent_decision: None,
        };
    }

    let intent_ctx = build_intent_routing_context(
        messages,
        cfg,
        in_clarification_flow,
        ExecuteIntentThresholds {
            low: cfg.intent_routing.intent_non_hier_execute_low_threshold,
            high: cfg.intent_routing.intent_non_hier_execute_high_threshold,
        },
    );
    let decision = assess_and_route(task.as_str(), &intent_ctx);
    let eligibility = staged_plan_eligibility_for_intent(task.as_str(), &decision);
    log_staged_gate_outcome(
        task.as_str(),
        &decision,
        "staged_plan_intent_gate_sync",
        eligibility,
    );

    match eligibility {
        Ok(()) => StagedPlanningGateOutcome::Allow {
            task_preview: task,
            intent_kind: decision.kind,
            primary_intent: decision.primary_intent.clone(),
            confidence: decision.confidence,
            decision,
        },
        Err(reason) => StagedPlanningGateOutcome::Deny {
            reason,
            task_preview: Some(task),
            intent_decision: Some(decision),
        },
    }
}
