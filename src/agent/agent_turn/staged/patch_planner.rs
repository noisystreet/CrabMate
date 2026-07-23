use log::{debug, warn};

use crate::agent::agent_turn::errors::{AgentTurnSubPhase, RunAgentTurnError};
use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::{self, AgentReplyPlanV1, PlanStepV1};
use crate::agent::reflection::plan_rewrite;
use crate::types::{Message, USER_CANCELLED_FINISH_REASON};

use super::super::params::RunLoopParams;
use super::planner_round_driver::{
    complete_planner_no_tools_chat_retrying, emit_staged_planner_tool_call_rejected_timeline,
};
use super::sse::{send_staged_plan_notice, staged_plan_queue_summary_text};
use super::staged_config_compat::{DEFAULT_STAGED_PLAN_BASELINE_MODE, StagedPlanBaselineMode};
use super::{StagedPlanRunLabels, prepare_staged_planner_no_tools_request};

/// 分阶段规划补丁轮入参（控制 clippy `too_many_arguments`）。
pub(super) struct StagedPlanPatchPlannerCtx<'p, 'a, F> {
    pub(super) p: &'p mut RunLoopParams<'a>,
    pub(super) per_coord: &'p mut PerCoordinator,
    pub(super) labels: &'p StagedPlanRunLabels,
    /// 仅用于补丁轮 `complete_chat_retrying`：CLI 可单独关闭规划模型 stdout。
    pub(super) planner_render_to_terminal: bool,
    pub(super) make_step_user_message: &'p F,
}

/// 分阶段步失败 → 补丁规划 **user** 文案参数（控制 `clippy::too_many_arguments`）。
pub(super) struct StagedPlanStepFailureFeedbackMeta<'a> {
    pub plan_id: &'a str,
    pub step_zero_based: usize,
    pub n_steps_total: usize,
    pub plan_patch_attempt_one_based: usize,
    pub plan_patch_budget: usize,
    pub reason_zh: &'a str,
    pub detail: &'a str,
    pub audit_counters_footer: &'a str,
}

pub(super) fn staged_plan_step_failure_feedback_user_body(
    meta: &StagedPlanStepFailureFeedbackMeta<'_>,
    step: &PlanStepV1,
) -> String {
    format!(
        "### 分阶段规划 · 步级反馈（plan_id={}）\n\
         **本步补丁规划尝试**：第 **{}/{}** 次（`staged_plan_patch_max_attempts` 约束的是**本失败分支**内可发起的补丁轮上界；与终答 **`plan_rewrite`** 计数**无关**）。\n\
         当前执行步 **{}/{}**（零基下标 {}）未顺利完成。\n\
         - 失败原因：{}\n\
         - 详情摘要：{}\n\
         - 当前步 id：`{}`\n\
         - 当前步描述：{}\n\n\
         请作为**规划器**仅输出一段可解析的 `agent_reply_plan` v1 JSON（可用 ```json 围栏）。\n\
         **补丁规则**：`steps` 数组表示从**本步起**的后续计划（可替换原剩余步骤、在末尾增加一步、或合并/拆分步骤）；须 **非空** 且 **不得** 使用 `no_task`。\n\
         已完成的前缀步（下标 0..{}）已由服务端保留，你**不要**在 `steps` 中重复列出。\n\n\
         Schema 须满足：{}\n\
         示例：\n```json\n{}\n```\n\n\
         ### 补丁规划补充（咨询/架构类用户问题）\n\
         若用户请求**主要是**架构意见、重构方向、隐式状态/风险点列举，而**不是**明确的代码落地、跑测或文档交付：后续 `steps` 应让用户尽快看到**直接结论**；避免再规划「仅为通读大量源文件」或「未经用户要求新建长篇设计文档」。可把剩余工作收口为**少量** `review_readonly` 步并在 `description` 中限定关键路径；**勿**在补丁 JSON 中使用 `no_task`（补丁规则要求非空 `steps`）。\n\
         {}",
        meta.plan_id,
        meta.plan_patch_attempt_one_based,
        meta.plan_patch_budget,
        meta.step_zero_based + 1,
        meta.n_steps_total,
        meta.step_zero_based,
        meta.reason_zh,
        meta.detail,
        step.id.trim(),
        step.description.trim(),
        meta.step_zero_based,
        plan_artifact::PLAN_V1_SCHEMA_RULES,
        plan_artifact::PLAN_V1_EXAMPLE_JSON,
        meta.audit_counters_footer
    )
}

/// 追加反馈 user 后跑一轮无工具规划；成功则返回合并后的 `steps`，失败返回 `Ok(None)`（调用方按补丁次数用尽处理）。
pub(super) async fn run_staged_plan_patch_planner_round<F>(
    ctx: &mut StagedPlanPatchPlannerCtx<'_, '_, F>,
    feedback_user_body: String,
    base_steps: &[PlanStepV1],
    failed_step_zero_based: usize,
) -> Result<Option<Vec<PlanStepV1>>, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let StagedPlanPatchPlannerCtx {
        p,
        per_coord,
        labels,
        planner_render_to_terminal,
        make_step_user_message,
    } = ctx;
    p.turn
        .push_message(make_step_user_message(feedback_user_body));
    let req = prepare_staged_planner_no_tools_request(p, per_coord, labels.build_planner_messages)
        .await?;
    let (mut msg, finish_reason) =
        complete_planner_no_tools_chat_retrying(p, &req, *planner_render_to_terminal).await?;

    debug!(
        target: "crabmate",
        "分阶段规划补丁轮 finish_reason={} assistant_preview={}",
        finish_reason,
        crate::redact::assistant_message_preview_for_log(&msg)
    );

    if finish_reason == USER_CANCELLED_FINISH_REASON {
        return Ok(None);
    }

    if let Some(tc) = msg.tool_calls.as_ref().filter(|c| !c.is_empty()) {
        debug!(
            target: "crabmate",
            "分阶段规划补丁轮：丢弃 API 返回的 {} 条原生 tool_calls，改从正文 DSML 物化",
            tc.len()
        );
    }
    let dsml_enabled = p
        .ctx
        .core
        .cfg
        .dsml_materialize
        .materialize_deepseek_dsml_tool_calls;
    let rejected =
        crate::dsml::staged_no_tools_materialized_count(&mut msg, dsml_enabled, "·补丁轮");

    p.turn.push_assistant_merging_trailing_empty(msg.clone());

    if rejected > 0 {
        emit_staged_planner_tool_call_rejected_timeline(p.ctx.io.out, rejected).await;
        warn!(
            target: "crabmate",
            "分阶段规划补丁轮：检测到 {} 条 tool_calls，严格无工具模式下拒绝并等待下次补丁重试",
            rejected
        );
        return Ok(None);
    }

    let validate_only_binding_ids =
        plan_rewrite::last_workflow_validate_binding_plan_node_ids(p.turn.messages());
    let patch_plan = match plan_artifact::parse_agent_reply_plan_v1_from_assistant_message_with_validate_only_binding_ids(
        &msg,
        validate_only_binding_ids.as_deref(),
    ) {
        Ok(plan_v1) => plan_v1,
        Err(e) => {
            warn!(
                target: "crabmate",
                "staged_plan_patch_invalid parse_err={}",
                plan_artifact::plan_artifact_error_log_summary(&e)
            );
            return Ok(None);
        }
    };

    match plan_artifact::merge_staged_plan_steps_after_step_failure(
        base_steps,
        &patch_plan,
        failed_step_zero_based,
    ) {
        Ok(merged) => {
            if DEFAULT_STAGED_PLAN_BASELINE_MODE == StagedPlanBaselineMode::StrictBaselineSteps
                && let Some(ref baseline) = p.turn.turn_planner_hints.staged_baseline_plan
                && let Err(e) = plan_artifact::validate_staged_patch_merged_strict_baseline_ids(
                    &baseline.steps,
                    &merged,
                    failed_step_zero_based,
                )
            {
                warn!(
                    target: "crabmate",
                    "staged_plan_patch_strict_baseline_rejected err={}",
                    plan_artifact::plan_artifact_error_log_summary(&e)
                );
                return Ok(None);
            }
            per_coord.record_staged_plan_patch_planner_round_completed();
            debug!(
                target: "crabmate",
                "staged_plan_patch_planner_ok merged_steps_len={} staged_patch_rounds_completed={} plan_rewrite_attempts={}",
                merged.len(),
                per_coord.staged_plan_patch_planner_rounds_snapshot(),
                per_coord.plan_rewrite_attempts_snapshot()
            );
            Ok(Some(merged))
        }
        Err(e) => {
            warn!(
                target: "crabmate",
                "staged_plan_patch_merge_failed err={}",
                plan_artifact::plan_artifact_error_log_summary(&e)
            );
            Ok(None)
        }
    }
}

/// 补丁合并结果与当前队列指纹一致时停止重试。
pub(crate) fn staged_patch_merged_plan_unchanged(
    before: &[PlanStepV1],
    merged: &[PlanStepV1],
) -> bool {
    plan_artifact::plan_steps_fingerprint(before) == plan_artifact::plan_steps_fingerprint(merged)
}

/// 补丁规划返回新 `steps` 后：写入 assistant JSON 并刷新队列 notice。
pub(crate) async fn push_patch_replan_assistant_json_and_notice(
    p: &mut RunLoopParams<'_>,
    plan_steps: &[PlanStepV1],
    echo_terminal_staged: bool,
    completed_steps_for_notice: usize,
) -> Result<(), RunAgentTurnError> {
    let replan = AgentReplyPlanV1 {
        plan_type: "agent_reply_plan".to_string(),
        version: 1,
        steps: plan_steps.to_vec(),
        no_task: false,
    };
    let json = plan_artifact::agent_reply_plan_v1_to_json_string(&replan).map_err(|e| {
        RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Executor,
            message: e.to_string(),
        }
    })?;
    p.turn
        .push_assistant_merging_trailing_empty(Message::assistant_only(json));
    send_staged_plan_notice(
        p.ctx.io.out,
        echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&replan, completed_steps_for_notice),
    )
    .await;
    Ok(())
}
