//! 分阶段规划与逻辑双 agent：规划轮 + 逐步注入执行。

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::atomic::Ordering;

use log::{debug, warn};
use tokio::sync::mpsc;

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::{self, AgentReplyPlanV1, PlanStepV1};
use crate::agent::plan_ensemble;
use crate::agent::plan_optimizer::{self, STAGED_PLAN_OPTIMIZER_COACH_MARK};
use crate::agent::reflection::plan_rewrite;
use crate::llm::{
    LlmCompleteError, LlmRetryingTransportOpts, kimi_k2_5_vendor_requires_tool_call_reasoning,
    no_tools_chat_request_from_messages,
};
use crate::sse::{SsePayload, encode_message};
use crate::tool_result::tool_message_content_ok_for_model;
use crate::types::{
    Message, USER_CANCELLED_FINISH_REASON, is_message_excluded_from_llm_context_except_memory,
    message_clone_stripping_reasoning_for_api,
    messages_for_api_stripping_reasoning_skip_ui_separators,
};

use super::errors::{AgentTurnSubPhase, RunAgentTurnError};
use super::execute_tools::sse_sender_closed;
use super::messages::push_assistant_merging_trailing_empty_placeholder;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;
use super::plan::agent_llm_call::AgentLlmCall;

mod ensemble_fsm;
mod ensemble_schedule_fsm;
mod full_pipeline_fsm;
mod orchestrator;
mod patch_planner;
mod planner_parse_fsm;
mod planner_round_fsm;
mod post_parse_pipeline_fsm;
mod prepared_parse_fsm;
mod prepared_post_parse_fsm;
mod sse;
mod staged_step_fsm;
mod step_iteration_fsm;
mod step_loop_fsm;
mod turn_fsm;

use orchestrator as staged_orchestrator;
use sse as staged_sse;

use ensemble_fsm::{
    EnsembleMergeOutcome, EnsembleSecondaryPlannerRoundOutcome,
    ensemble_merge_outcome_from_parsed_steps, ensemble_secondary_planner_round_outcome,
};
use ensemble_schedule_fsm::{
    EnsembleDriverPhase, ensemble_merge_should_run, ensemble_secondary_planner_display_index,
    resolve_ensemble_driver_phase,
};
use full_pipeline_fsm::{
    StagedFullPipelinePhase, debug_staged_full_pipeline_enter,
    debug_staged_full_pipeline_transition,
};
use patch_planner::{
    StagedPlanPatchPlannerCtx, StagedPlanStepFailureFeedbackMeta,
    run_staged_plan_patch_planner_round, staged_plan_step_failure_feedback_user_body,
};
use planner_parse_fsm::omit_no_task_planner_from_history;
use post_parse_pipeline_fsm::{
    ensemble_merge_should_invoke, ensemble_merge_skip_for_casual_prompt,
    log_staged_plan_ensemble_route, log_staged_plan_optimizer_route, optimizer_round_should_run,
};
use prepared_parse_fsm::{PreparedPlannerRoute, resolve_prepared_planner_route};
use prepared_post_parse_fsm::{
    PreparedFullPipelineInputs, PreparedFullPipelineSchedule, PreparedPostParseSchedule,
    prepared_full_pipeline_schedule, prepared_post_parse_schedule,
};
use staged_sse::{
    emit_chat_ui_separator_sse, next_staged_plan_id, send_staged_plan_finished,
    send_staged_plan_notice, send_staged_plan_step_finished, send_staged_plan_step_started,
    staged_plan_nl_followup_user_body, staged_plan_phase_instruction_default,
    staged_plan_queue_summary_text,
};
use staged_step_fsm::{
    staged_patch_budget_after_step_failure, staged_patch_budget_tool_messages_not_ok,
    staged_step_patch_planner_enabled,
};
use step_iteration_fsm::{
    STAGED_STEP_OUTER_LOOP_FAIL_DETAIL, STAGED_STEP_TOOL_MSG_FAIL_DETAIL, StagedStepAfterOuterLoop,
    StagedStepToolPhaseRoute, staged_step_after_outer_loop,
    staged_step_failure_retry_exhausted_message, staged_step_tool_phase_route,
    staged_step_verify_fail_patch_detail, staged_step_wall_clock_exceeded,
};
use step_loop_fsm::{staged_injected_step_user_body, try_apply_staged_plan_control_flow_jump};
use turn_fsm::{
    StagedTurnAdvance, StagedTurnPhase, StagedTurnSubCallOutcome,
    entered_flag_for_next_planner_call, staged_rolling_horizon_apply_advance,
};

/// 首轮规划 assistant：清空原生 tool_calls 后经 DSML 物化，返回「等价 tool_calls 条数」总和（用于判定是否触发一次重写 user）。
fn staged_first_planner_round_tool_call_total_after_materialize(
    msg: &mut Message,
    materialize_deepseek_dsml_tool_calls: bool,
) -> usize {
    let raw_count = msg.tool_calls.as_ref().map(|c| c.len()).unwrap_or(0);
    msg.tool_calls = None;
    crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(
        msg,
        materialize_deepseek_dsml_tool_calls,
    );
    let dsml_count = msg.tool_calls.as_ref().map(|c| c.len()).unwrap_or(0);
    raw_count.saturating_add(dsml_count)
}

fn staged_planner_tool_call_reject_user_body(tool_call_count: usize) -> String {
    format!(
        "### 规划轮约束提醒（code=PLANNER_TOOL_CALL_REJECTED）\n\
         你在无工具规划轮中输出了 {tool_call_count} 条 tool_calls，但本轮严格禁止工具调用。\n\
         请立即重写并仅输出一段可解析的 `agent_reply_plan` v1 JSON（可用 ```json 围栏），不要包含 tool_calls、DSML 或任何函数调用片段。"
    )
}

async fn emit_staged_planner_tool_call_rejected_timeline(
    out: Option<&mpsc::Sender<String>>,
    count: usize,
) {
    let Some(tx) = out else {
        return;
    };
    let detail = format!(
        "code=PLANNER_TOOL_CALL_REJECTED; rejected_tool_calls={count}; action=planner_rewrite_once"
    );
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::TimelineLog {
            log: crate::sse::TimelineLogBody {
                kind: "planner_tool_call_rejected".to_string(),
                title: "规划轮工具调用已拒绝".to_string(),
                detail: Some(detail),
            },
        }),
        "staged::planner_tool_call_rejected_timeline",
    )
    .await;
}

/// 若最后一条为带「规划教练」标记的临时 user，则弹出（取消或解析失败时避免孤立上下文）。
fn pop_last_staged_planner_coach_user_if_present(messages: &mut Vec<Message>) {
    if let Some(last) = messages.last()
        && last.role == "user"
        && crate::types::message_content_as_str(&last.content).is_some_and(|c| {
            c.contains(STAGED_PLAN_OPTIMIZER_COACH_MARK)
                || plan_ensemble::is_ensemble_injected_user_content(c)
        })
    {
        messages.pop();
    }
}

/// 两阶段 NL 开启时：无工具规划轮不向 Web/终端流式下发（由 NL 补全轮承担用户可见输出）。
fn staged_planner_sse_fully_suppressed(cfg: &crate::config::AgentConfig) -> bool {
    cfg.staged_plan_two_phase_nl_display
}

/// 无工具规划轮 `complete_chat_retrying`：
/// - **两阶段 NL**：`out: None`（整段抑制）；
/// - **Web + 未** `CM_WEB_RAW_ASSISTANT_OUTPUT`：经 [`super::plan::PlannerSseGate`] — 解析（正文+思维链）为 `no_task` 则整轮不落 SSE，且不将本条 assistant 写入会话；否则仅落 `assistant_answer_phase` 之后的正文增量；
/// - **RAW** 或 **非 Web**：`out: p.ctx.out`（整段原样下发）。
async fn complete_planner_no_tools_chat_retrying(
    p: &RunLoopParams<'_>,
    req: &crate::types::ChatRequest,
    planner_render_to_terminal: bool,
) -> Result<(Message, String), LlmCompleteError> {
    let suppress_full = staged_planner_sse_fully_suppressed(p.ctx.cfg.as_ref());
    let use_gate = p.ctx.out.is_some()
        && !crate::web::web_ui_env::web_raw_assistant_output_env()
        && !suppress_full;

    let gate_opt = match (use_gate, p.ctx.out.as_ref()) {
        (true, Some(out)) => Some(super::plan::PlannerSseGate::spawn((*out).clone())),
        _ => None,
    };

    let out_ref: Option<&mpsc::Sender<String>> = if suppress_full {
        None
    } else if let Some(ref g) = gate_opt {
        Some(&g.inner_tx)
    } else {
        p.ctx.out
    };

    let llm = AgentLlmCall::new(p);
    let res = llm
        .complete_retrying(
            LlmRetryingTransportOpts {
                out: out_ref,
                render_to_terminal: planner_render_to_terminal && !suppress_full,
                no_stream: p.ctx.no_stream,
                cancel: p.ctx.cancel,
                plain_terminal_stream: p.ctx.plain_terminal_stream,
            },
            req,
        )
        .await?;
    if let Some(gate) = gate_opt {
        gate.finish(&res.0).await;
    }
    Ok(res)
}

/// `staged_plan_two_phase_nl_display` 开启时：在上下文中已有规划 JSON 助手条后，追加一轮仅自然语言的**用户可见**输出（SSE/终端）。
async fn run_staged_plan_nl_followup_round<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    make_step_user_message: &F,
) -> Result<(), RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let mark = p.turn.messages.len();
    p.turn
        .messages
        .push(make_step_user_message(staged_plan_nl_followup_user_body()));
    let result: Result<(), RunAgentTurnError> = async {
        crate::agent::context_window::prepare_messages_for_model(
            p.ctx.llm_backend,
            p.ctx.client,
            p.ctx.api_key,
            p.ctx.cfg.as_ref(),
            p.turn.messages,
            Some(per_coord),
            p.ctx.workspace_changelist.as_ref().map(|a| a.as_ref()),
        )
        .await
        .map_err(|e| RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: e.to_string(),
        })?;
        let stripped = messages_for_api_stripping_reasoning_skip_ui_separators(
            p.turn.messages.as_slice(),
            kimi_k2_5_vendor_requires_tool_call_reasoning(p.ctx.cfg.as_ref()),
            crate::llm::vendor::deepseek_json_output_eligible(p.ctx.cfg.as_ref()),
        );
        let req = no_tools_chat_request_from_messages(
            p.ctx.cfg.as_ref(),
            stripped,
            p.turn.temperature_override,
            p.effective_model(),
            p.turn.seed_override,
        );
        let llm = AgentLlmCall::new(p);
        let (mut msg, finish_reason) = llm.complete_retrying(p.llm_transport_opts(), &req).await?;
        if finish_reason == USER_CANCELLED_FINISH_REASON {
            p.turn.messages.pop();
            return Ok(());
        }
        if let Some(tc) = msg.tool_calls.as_ref().filter(|c| !c.is_empty()) {
            debug!(
                target: "crabmate",
                "分阶段规划·自然语言补全轮：丢弃 API 返回的 {} 条原生 tool_calls",
                tc.len()
            );
        }
        msg.tool_calls = None;
        crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(
            &mut msg,
            p.ctx.cfg.materialize_deepseek_dsml_tool_calls,
        );
        if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
            warn!(
                target: "crabmate",
                "分阶段规划·自然语言补全轮：DSML 物化出 tool_calls，已忽略"
            );
            msg.tool_calls = None;
        }
        push_assistant_merging_trailing_empty_placeholder(p.turn.messages, msg);
        Ok(())
    }
    .await;
    if result.is_err() && p.turn.messages.len() > mark {
        p.turn.messages.truncate(mark);
    }
    result
}

/// 无工具规划补全：假定 `p.turn.messages` 已含本轮所需的 user（若有）；与 `prepare_staged_planner_no_tools_request` + `complete_planner_no_tools_chat_retrying` 一致。
async fn complete_one_staged_planner_assistant_round(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    build_planner_messages: fn(&[Message], String, bool, bool) -> Vec<Message>,
    planner_render_to_terminal: bool,
    log_label: &'static str,
) -> Result<(Message, String), RunAgentTurnError> {
    let req = prepare_staged_planner_no_tools_request(p, per_coord, build_planner_messages).await?;
    let (msg, finish_reason) =
        complete_planner_no_tools_chat_retrying(p, &req, planner_render_to_terminal).await?;
    debug!(
        target: "crabmate",
        "{} finish_reason={} assistant_preview={}",
        log_label,
        finish_reason,
        crate::redact::assistant_message_preview_for_log(&msg)
    );
    Ok((msg, finish_reason))
}

/// 与首轮/优化轮一致：忽略原生 tool_calls，物化 DSML 后再清空，仅解析正文规划 JSON。
fn strip_staged_planner_message_tool_calls(
    msg: &mut Message,
    round_hint: &'static str,
    dsml: bool,
) {
    if let Some(tc) = msg.tool_calls.as_ref().filter(|c| !c.is_empty()) {
        debug!(
            target: "crabmate",
            "分阶段规划{round_hint}：丢弃 API 返回的 {} 条原生 tool_calls，改从正文解析",
            tc.len()
        );
    }
    msg.tool_calls = None;
    crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(msg, dsml);
    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        warn!(
            target: "crabmate",
            "分阶段规划{round_hint}：正文物化出 tool_calls，已忽略，仅尝试从正文解析规划 JSON"
        );
        msg.tool_calls = None;
    }
}

/// 逻辑多规划员（串行）+ 合并：首轮规划已在历史中；辅助规划员轮**不**写入 assistant，以免上下文膨胀。
async fn maybe_run_staged_plan_ensemble_then_merge<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    labels: &StagedPlanRunLabels,
    make_step_user_message: &F,
    planner_render_to_terminal: bool,
    plan: &mut AgentReplyPlanV1,
    skip_for_casual_user_prompt: bool,
) -> Result<(), RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let phase = resolve_ensemble_driver_phase(
        p.ctx.staged_plan_ensemble_count,
        skip_for_casual_user_prompt,
    );
    let EnsembleDriverPhase::SecondaryChain { extra } = phase else {
        if skip_for_casual_user_prompt {
            debug!(
                target: "crabmate",
                "分阶段规划·逻辑多规划员：用户输入偏短/寒暄启发式，跳过 ensemble（staged_plan_ensemble_count={}）以省 API",
                p.ctx.staged_plan_ensemble_count
            );
        }
        return Ok(());
    };

    let dsml = p.ctx.cfg.materialize_deepseek_dsml_tool_calls;
    let mut accepted: Vec<AgentReplyPlanV1> = vec![plan.clone()];

    for i in 0..extra {
        let planner_idx = ensemble_secondary_planner_display_index(i);
        let body = plan_ensemble::ensemble_secondary_planner_user_body(planner_idx, &accepted);
        p.turn.messages.push(make_step_user_message(body));
        let (mut sec_msg, fin) = complete_one_staged_planner_assistant_round(
            p,
            per_coord,
            labels.build_planner_messages,
            planner_render_to_terminal,
            "分阶段规划·逻辑多规划员轮",
        )
        .await?;
        if fin == USER_CANCELLED_FINISH_REASON {
            pop_last_staged_planner_coach_user_if_present(p.turn.messages);
            return Ok(());
        }
        strip_staged_planner_message_tool_calls(&mut sec_msg, "·逻辑多规划员", dsml);
        let validate_only_binding_ids =
            plan_rewrite::last_workflow_validate_binding_plan_node_ids(p.turn.messages);
        let parsed =
            plan_artifact::parse_agent_reply_plan_v1_from_assistant_message_with_validate_only_binding_ids(
                &sec_msg,
                validate_only_binding_ids.as_deref(),
            );
        match ensemble_secondary_planner_round_outcome(parsed) {
            EnsembleSecondaryPlannerRoundOutcome::AcceptAppend(p2) => {
                pop_last_staged_planner_coach_user_if_present(p.turn.messages);
                accepted.push(p2);
            }
            EnsembleSecondaryPlannerRoundOutcome::StopChain => {
                warn!(
                    target: "crabmate",
                    "分阶段规划·逻辑多规划员：第 {} 份规划解析失败或无效，停止追加规划员（保留已收集的 {} 份）",
                    planner_idx,
                    accepted.len()
                );
                pop_last_staged_planner_coach_user_if_present(p.turn.messages);
                break;
            }
        }
    }

    if !ensemble_merge_should_run(accepted.len()) {
        return Ok(());
    }

    let merge_body = plan_ensemble::ensemble_merge_planner_user_body(&accepted);
    p.turn.messages.push(make_step_user_message(merge_body));
    let (mut merge_msg, merge_fin) = complete_one_staged_planner_assistant_round(
        p,
        per_coord,
        labels.build_planner_messages,
        planner_render_to_terminal,
        "分阶段规划·多规划合并轮",
    )
    .await?;
    if merge_fin == USER_CANCELLED_FINISH_REASON {
        pop_last_staged_planner_coach_user_if_present(p.turn.messages);
        return Ok(());
    }
    strip_staged_planner_message_tool_calls(&mut merge_msg, "·多规划合并", dsml);
    let merge_content = plan_artifact::assistant_merged_text_for_plan_artifact_parse(&merge_msg);
    let merged_steps = plan_ensemble::try_parse_ensemble_planner_reply(&merge_content);
    match ensemble_merge_outcome_from_parsed_steps(merged_steps) {
        EnsembleMergeOutcome::AppliedSteps(steps) => {
            debug!(
                target: "crabmate",
                "分阶段规划·多规划合并：步数 {} -> {}（来自 {} 份草案）",
                plan.steps.len(),
                steps.len(),
                accepted.len()
            );
            push_assistant_merging_trailing_empty_placeholder(p.turn.messages, merge_msg);
            plan.steps = steps;
        }
        EnsembleMergeOutcome::KeepPriorPlan => {
            warn!(
                target: "crabmate",
                "分阶段规划·多规划合并：未解析出合法 agent_reply_plan，沿用合并前规划（{} 步）",
                plan.steps.len()
            );
            pop_last_staged_planner_coach_user_if_present(p.turn.messages);
        }
    }
    Ok(())
}

/// 分阶段规划共享执行路径上的日志文案（避免 `run_staged_plan_with_prepared_request` 参数过长）。
#[derive(Clone, Copy)]
pub(crate) struct StagedPlanRunLabels {
    pub planning_log_label: &'static str,
    pub step_injection_log_label: &'static str,
    pub build_planner_messages: fn(&[Message], String, bool, bool) -> Vec<Message>,
}

async fn prepare_staged_planner_no_tools_request(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    build_planner_messages: fn(&[Message], String, bool, bool) -> Vec<Message>,
) -> Result<crate::types::ChatRequest, RunAgentTurnError> {
    if let Some(ref ltm) = p.ctx.long_term_memory {
        ltm.prepare_messages(
            p.ctx.cfg.as_ref(),
            p.ctx.long_term_memory_scope_id.as_deref(),
            p.turn.messages,
        );
    }
    crate::agent::context_window::prepare_messages_for_model(
        p.ctx.llm_backend,
        p.ctx.client,
        p.ctx.api_key,
        p.ctx.cfg.as_ref(),
        p.turn.messages,
        Some(per_coord),
        p.ctx.workspace_changelist.as_ref().map(|a| a.as_ref()),
    )
    .await
    .map_err(|e| RunAgentTurnError::Other {
        phase: AgentTurnSubPhase::Planner,
        message: e.to_string(),
    })?;

    let instr = p.ctx.cfg.staged_plan_phase_instruction.trim();
    let plan_system = if instr.is_empty() {
        staged_plan_phase_instruction_default()
    } else {
        instr.to_string()
    };
    let preserve_kimi = crate::llm::llm_vendor_adapter(p.ctx.cfg.as_ref())
        .preserve_assistant_tool_call_reasoning(p.ctx.cfg.as_ref());
    let preserve_deepseek = crate::llm::vendor::deepseek_json_output_eligible(p.ctx.cfg.as_ref());
    Ok(no_tools_chat_request_from_messages(
        p.ctx.cfg.as_ref(),
        build_planner_messages(
            p.turn.messages,
            plan_system,
            preserve_kimi,
            preserve_deepseek,
        ),
        p.turn.temperature_override,
        p.effective_model(),
        p.turn.seed_override,
    ))
}

/// 滚动视界外层循环变体（与 [`advance_staged_turn_after_sub_call`]、`StagedTurnPhase` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StagedRollingHorizonKind {
    SingleAgent,
    LogicalDualAgent,
}

impl StagedRollingHorizonKind {
    fn max_rounds_error_message(self, cap: usize) -> String {
        match self {
            Self::SingleAgent => format!(
                "分阶段单步规划轮次超过上限（{}），已停止以避免无限循环",
                cap
            ),
            Self::LogicalDualAgent => format!(
                "逻辑双Agent分阶段单步规划轮次超过上限（{}），已停止以避免无限循环",
                cap
            ),
        }
    }

    fn snapshot_rollback_warn_summary(self) -> &'static str {
        match self {
            Self::SingleAgent => "工作区快照回滚失败",
            Self::LogicalDualAgent => "逻辑双Agent快照回滚失败",
        }
    }
}

/// 单 agent / 逻辑双 agent 共用的 **滚动视界** 外层循环：`turn_fsm` 相位 + 子调用结果 → `StagedTurnAdvance`。
///
/// 见 `docs/design/per_state_machine_consolidation.md` §3.2（分阶段回合编排）；真实转移表在 [`advance_staged_turn_after_sub_call`]。
#[allow(clippy::too_many_arguments)]
async fn run_staged_rolling_horizon_outer_loop<F>(
    kind: StagedRollingHorizonKind,
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    labels: StagedPlanRunLabels,
    render_to_terminal: bool,
    echo_terminal_staged: bool,
    make_step_user_message: F,
) -> Result<(), RunAgentTurnError>
where
    F: Fn(String) -> Message + Copy,
{
    let mut rewrite_attempts = 0;
    let max_rewrites = p.ctx.cfg.full_plan_rewrite_max_attempts;
    let mut phase = StagedTurnPhase::PreStepExecutionRound;
    let mut staged_rounds = 0usize;
    const STAGED_SINGLE_STEP_MAX_ROUNDS: usize = 64;
    let snapshot =
        crate::agent::workspace_snapshot::WorkspaceSnapshot::take(p.ctx.effective_working_dir);

    loop {
        staged_rounds = staged_rounds.saturating_add(1);
        if staged_rounds > STAGED_SINGLE_STEP_MAX_ROUNDS {
            tracing::warn!(
                target: "crabmate::staged",
                staged_fsm = "rolling_horizon",
                rolling_horizon_kind = ?kind,
                staged_round = staged_rounds,
                staged_turn_phase = ?phase,
                sub_phase = "planner",
                "staged rolling horizon exceeded max rounds"
            );
            return Err(RunAgentTurnError::Other {
                phase: AgentTurnSubPhase::Planner,
                message: kind.max_rounds_error_message(STAGED_SINGLE_STEP_MAX_ROUNDS),
            });
        }

        tracing::debug!(
            target: "crabmate::staged",
            staged_fsm = "rolling_horizon",
            rolling_horizon_kind = ?kind,
            staged_round = staged_rounds,
            staged_turn_phase = ?phase,
            rewrite_attempts = rewrite_attempts,
            sub_phase = "planner",
            "staged rolling horizon iteration enter"
        );

        let req =
            prepare_staged_planner_no_tools_request(p, per_coord, labels.build_planner_messages)
                .await?;
        let entered_from_step_execution_round = entered_flag_for_next_planner_call(phase);
        let res = run_staged_plan_with_prepared_request(
            p,
            per_coord,
            req,
            render_to_terminal,
            echo_terminal_staged,
            entered_from_step_execution_round,
            labels,
            make_step_user_message,
        )
        .await;

        let event = match res {
            Ok(o) => StagedTurnSubCallOutcome::Ok(o),
            Err(e) => StagedTurnSubCallOutcome::Err(e),
        };
        let step =
            staged_rolling_horizon_apply_advance(phase, rewrite_attempts, max_rewrites, event);
        rewrite_attempts = step.next_rewrite_attempts;

        tracing::debug!(
            target: "crabmate::staged",
            staged_fsm = "rolling_horizon",
            rolling_horizon_kind = ?kind,
            staged_round = staged_rounds,
            prior_staged_turn_phase = ?phase,
            advance_kind = step.advance_kind,
            propagate_public_code = step.propagate_public_code,
            rewrite_attempts = rewrite_attempts,
            sub_phase = "planner",
            "staged rolling horizon advance"
        );

        match step.advance {
            StagedTurnAdvance::Continue {
                phase: next_phase,
                push_feedback_user,
            } => {
                phase = next_phase;
                if let Some(u) = push_feedback_user {
                    if let Some(ref snap) = snapshot {
                        if let Err(e) = snap.restore() {
                            tracing::warn!(
                                target: "crabmate::staged",
                                staged_fsm = "rolling_horizon",
                                rolling_horizon_kind = ?kind,
                                summary = kind.snapshot_rollback_warn_summary(),
                                error = %e,
                                sub_phase = "planner",
                                "workspace snapshot rollback failed"
                            );
                        } else {
                            tracing::info!(
                                target: "crabmate::staged",
                                staged_fsm = "rolling_horizon",
                                rolling_horizon_kind = ?kind,
                                sub_phase = "planner",
                                "global replan triggered; workspace snapshot restored"
                            );
                        }
                    }
                    p.turn.messages.push(u);
                } else if matches!(phase, StagedTurnPhase::AfterStepExecutionRound) {
                    tracing::debug!(
                        target: "crabmate::staged",
                        staged_fsm = "rolling_horizon",
                        rolling_horizon_kind = ?kind,
                        staged_round = staged_rounds,
                        staged_turn_phase = ?phase,
                        outcome = "continue_after_step",
                        sub_phase = "planner",
                        "step execution round completed; next no-tools planner round"
                    );
                }
                continue;
            }
            StagedTurnAdvance::Finished => return Ok(()),
            StagedTurnAdvance::ReplanExhausted { phase: ph, message } => {
                return Err(RunAgentTurnError::ReplanExhausted { phase: ph, message });
            }
            StagedTurnAdvance::Propagate(e) => return Err(e),
        }
    }
}

pub(super) async fn run_staged_plan_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    let render_to_terminal = p.ctx.render_to_terminal;
    let echo_terminal_staged = render_to_terminal && p.ctx.out.is_none();

    let labels = StagedPlanRunLabels {
        planning_log_label: "分阶段规划轮模型输出",
        step_injection_log_label: "分步注入 user（完整正文，供排障与日志）",
        build_planner_messages: build_single_agent_planner_messages,
    };

    run_staged_rolling_horizon_outer_loop(
        StagedRollingHorizonKind::SingleAgent,
        p,
        per_coord,
        labels,
        render_to_terminal,
        echo_terminal_staged,
        |body| Message {
            role: "user".to_string(),
            content: Some(body.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    )
    .await
}

pub(crate) fn build_single_agent_planner_messages(
    messages: &[Message],
    plan_system: String,
    preserve_reasoning_on_assistant_tool_calls: bool,
    preserve_deepseek_thinking_reasoning_roundtrip: bool,
) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_message_excluded_from_llm_context_except_memory(m))
        .map(|m| {
            message_clone_stripping_reasoning_for_api(
                m,
                preserve_reasoning_on_assistant_tool_calls,
                preserve_deepseek_thinking_reasoning_roundtrip,
            )
        })
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

pub(crate) fn build_logical_dual_planner_messages(
    messages: &[Message],
    plan_system: String,
    preserve_reasoning_on_assistant_tool_calls: bool,
    preserve_deepseek_thinking_reasoning_roundtrip: bool,
) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_message_excluded_from_llm_context_except_memory(m))
        // 逻辑双 agent：规划器只看用户/助手自然语言上下文，不看 tool 结果正文，
        // 避免工具细节污染任务拆解。
        .filter(|m| m.role != "tool")
        .filter(|m| {
            if m.role != "assistant" {
                return true;
            }
            crate::types::message_content_as_str(&m.content)
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        })
        .map(|m| {
            message_clone_stripping_reasoning_for_api(
                m,
                preserve_reasoning_on_assistant_tool_calls,
                preserve_deepseek_thinking_reasoning_roundtrip,
            )
        })
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlanRunOutcome {
    ContinuePlanning,
    Finished,
}

#[cfg(test)]
pub(crate) fn simulate_single_step_rolling_horizon_for_test(
    outcomes: &[StagedPlanRunOutcome],
    max_rounds: usize,
) -> Result<usize, String> {
    let mut staged_rounds = 0usize;
    let mut idx = 0usize;
    loop {
        staged_rounds = staged_rounds.saturating_add(1);
        if staged_rounds > max_rounds {
            return Err(format!(
                "分阶段单步规划轮次超过上限（{}），已停止以避免无限循环",
                max_rounds
            ));
        }
        let outcome = outcomes
            .get(idx)
            .copied()
            .unwrap_or(StagedPlanRunOutcome::ContinuePlanning);
        idx = idx.saturating_add(1);
        match outcome {
            StagedPlanRunOutcome::ContinuePlanning => continue,
            StagedPlanRunOutcome::Finished => return Ok(staged_rounds),
        }
    }
}

#[cfg(test)]
mod staged_rolling_horizon_kind_tests {
    use super::StagedRollingHorizonKind;

    #[test]
    fn max_rounds_error_messages_distinct_by_variant() {
        let a = StagedRollingHorizonKind::SingleAgent.max_rounds_error_message(64);
        let b = StagedRollingHorizonKind::LogicalDualAgent.max_rounds_error_message(64);
        assert_ne!(a, b);
        assert!(a.contains("分阶段单步"), "{a}");
        assert!(b.contains("逻辑双Agent"), "{b}");
    }
}

/// 发送单步结束 SSE（`failed` / `cancelled` / `ok`）。
#[allow(clippy::too_many_arguments)]
async fn finish_staged_plan_step_sse(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id_trim: &str,
    step_index: usize,
    n: usize,
    status: &'static str,
    executor_kind: Option<crate::agent::plan_artifact::PlanStepExecutorKind>,
    verify_fail_reason: Option<&str>,
) {
    send_staged_plan_step_finished(
        out,
        plan_id,
        step_id_trim,
        step_index,
        n,
        status,
        executor_kind.map(|k| k.as_snake_case_str()),
        verify_fail_reason,
    )
    .await;
}

/// 执行步失败早退：`step_finished(failed)` + `plan_finished(failed)`，避免漏发 `staged_plan_finished`。
struct StagedPlanStepFailedExit<'a> {
    out: Option<&'a mpsc::Sender<String>>,
    plan_id: &'a str,
    step_id_trim: &'a str,
    step_index: usize,
    n: usize,
    completed_steps_before_this: usize,
}

async fn finish_staged_plan_step_failed_and_plan_failed_sse(
    f: StagedPlanStepFailedExit<'_>,
    executor_kind: Option<crate::agent::plan_artifact::PlanStepExecutorKind>,
    verify_fail_reason: Option<&str>,
) {
    finish_staged_plan_step_sse(
        f.out,
        f.plan_id,
        f.step_id_trim,
        f.step_index,
        f.n,
        "failed",
        executor_kind,
        verify_fail_reason,
    )
    .await;
    send_staged_plan_finished(
        f.out,
        f.plan_id,
        f.n,
        f.completed_steps_before_this,
        "failed",
    )
    .await;
}

/// 自本步 user 注入起至下一条 user（或历史末尾）之间的 `role: tool` 是否均为成功（与信封 `ok` / 传统解析一致）。
fn staged_step_tool_messages_all_ok(messages: &[Message], step_user_index: usize) -> bool {
    let mut i = step_user_index.saturating_add(1);
    while i < messages.len() {
        let m = &messages[i];
        if m.role == "user" {
            break;
        }
        if m.role == "tool" {
            let content = crate::types::message_content_as_str(&m.content).unwrap_or("");
            if !tool_message_content_ok_for_model(content, "") {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 补丁规划返回新 `steps` 后：写入 assistant JSON（围栏）并刷新队列 notice（两处失败路径共用）。
async fn push_patch_replan_assistant_json_and_notice(
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
    push_assistant_merging_trailing_empty_placeholder(
        p.turn.messages,
        Message::assistant_only(json),
    );
    send_staged_plan_notice(
        p.ctx.out,
        echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&replan, completed_steps_for_notice),
    )
    .await;
    Ok(())
}

/// 单次 `run_staged_plan_steps_loop` 迭代结束方式（不含墙钟：由外层检查）。
enum StagedStepIterationCtl {
    /// 补丁重规划后重试当前下标（`i` 不变）。
    RetryCurrentStep { n: usize },
    /// 本步已完结（transition 或成功），调用方将 `i += 1`。
    AdvanceToNextStep { n: usize, completed_steps: usize },
    /// 本步成功后检测到取消（与历史：先发 `step_finished(cancelled)` 再 `break`）。
    CancelledAfterOuterOk,
}

/// outer_loop 与验收之后、transition / 补丁 / 工具检查 / 成功收尾 之前的数据（**AfterOuterLoop** 阶段入参）。
struct StagedStepOuterHalfResult {
    step: PlanStepV1,
    step_index: usize,
    step_user_idx: usize,
    run_step: Result<(), RunAgentTurnError>,
    step_verify_failed_reason: Option<String>,
}

struct StagedStepRunOuterHalfParams<'a, 'b, 'c, F> {
    plan_id: &'a str,
    i: usize,
    n: usize,
    plan_steps: &'a [PlanStepV1],
    echo_terminal_staged: bool,
    labels: &'a StagedPlanRunLabels,
    patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
    make_step_user_message: &'a F,
}

/// **`StagedStepRunningSub::BeforeStepLlm`** → **`InOuterLoop`**：发 `step_started`、注入 user、`run_agent_outer_loop`、可选 acceptance。
async fn staged_step_run_outer_half<F>(
    p: StagedStepRunOuterHalfParams<'_, '_, '_, F>,
) -> StagedStepOuterHalfResult
where
    F: Fn(String) -> Message,
{
    let StagedStepRunOuterHalfParams {
        plan_id,
        i,
        n,
        plan_steps,
        echo_terminal_staged,
        labels,
        patch_ctx,
        make_step_user_message,
    } = p;
    let step = plan_steps[i].clone();
    let step_index = i + 1;
    send_staged_plan_step_started(
        patch_ctx.p.ctx.out,
        plan_id,
        step.id.trim(),
        step_index,
        n,
        step.description.trim(),
        step.executor_kind.map(|k| k.as_snake_case_str()),
    )
    .await;

    let body = staged_injected_step_user_body(step_index, n, &step);
    debug!(
        target: "crabmate",
        "{} step={}/{} body_len={} body_preview={}",
        labels.step_injection_log_label,
        i + 1,
        n,
        body.len(),
        crate::redact::preview_chars(&body, crate::redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    if echo_terminal_staged {
        let _ = crate::runtime::terminal_cli_transcript::print_staged_plan_notice(false, &body);
    }
    let step_user_idx = patch_ctx.p.turn.messages.len();
    patch_ctx.p.turn.messages.push(make_step_user_message(body));
    let prev_executor_constraint = patch_ctx.p.turn.step_executor_constraint;
    patch_ctx.p.turn.step_executor_constraint = step.executor_kind;
    let run_step = run_agent_outer_loop(patch_ctx.p, patch_ctx.per_coord).await;
    patch_ctx.p.turn.step_executor_constraint = prev_executor_constraint;

    let mut step_verify_failed_reason: Option<String> = None;
    if run_step.is_ok() {
        #[allow(clippy::collapsible_if)]
        if let Some(ref acceptance) = step.acceptance {
            let verify_result = crate::agent::step_verifier::verify_step_execution(
                acceptance,
                patch_ctx.p.turn.messages,
                step_user_idx,
                patch_ctx.p.ctx.effective_working_dir,
            );

            if let crate::agent::step_verifier::VerifyResult::Fail { reason } = verify_result {
                step_verify_failed_reason = Some(reason);
            }
        }
    }

    StagedStepOuterHalfResult {
        step,
        step_index,
        step_user_idx,
        run_step,
        step_verify_failed_reason,
    }
}

/// **`StagedStepRunningSub::AfterOuterLoop`**：transition、失败补丁、取消、工具补丁、成功 SSE。
struct StagedStepRunAfterOuterHalfParams<'a, 'b, 'c, F> {
    outer: StagedStepOuterHalfResult,
    plan_id: &'a str,
    i: usize,
    n: usize,
    completed_steps: usize,
    plan_steps: &'a mut Vec<PlanStepV1>,
    original_steps: &'a [PlanStepV1],
    transition_counters: &'a mut HashMap<String, u32>,
    echo_terminal_staged: bool,
    patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
}

async fn staged_step_run_after_outer_half<F>(
    p: StagedStepRunAfterOuterHalfParams<'_, '_, '_, F>,
) -> Result<StagedStepIterationCtl, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let StagedStepRunAfterOuterHalfParams {
        outer,
        plan_id,
        i,
        mut n,
        completed_steps,
        plan_steps,
        original_steps,
        transition_counters,
        echo_terminal_staged,
        patch_ctx,
    } = p;
    let StagedStepOuterHalfResult {
        step,
        step_index,
        step_user_idx,
        run_step,
        step_verify_failed_reason,
    } = outer;

    if let Some((fb, step_status)) = try_apply_staged_plan_control_flow_jump(
        &step,
        i,
        plan_steps,
        original_steps,
        transition_counters,
        run_step.is_err() || step_verify_failed_reason.is_some(),
        &step_verify_failed_reason,
    ) {
        n = plan_steps.len();

        patch_ctx.p.turn.messages.push(Message::user_only(fb));

        let replan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".to_string(),
            version: 1,
            steps: plan_steps.clone(),
            no_task: false,
        };
        send_staged_plan_notice(
            patch_ctx.p.ctx.out,
            echo_terminal_staged,
            true,
            staged_plan_queue_summary_text(&replan, completed_steps),
        )
        .await;

        let step_verify_fail_reason = step_verify_failed_reason.as_deref();
        finish_staged_plan_step_sse(
            patch_ctx.p.ctx.out,
            plan_id,
            step.id.trim(),
            step_index,
            n,
            step_status,
            step.executor_kind,
            step_verify_fail_reason,
        )
        .await;
        patch_ctx
            .p
            .turn
            .messages
            .push(Message::chat_ui_separator(true));
        emit_chat_ui_separator_sse(patch_ctx.p.ctx.out, true).await;
        return Ok(StagedStepIterationCtl::AdvanceToNextStep {
            n,
            completed_steps: step_index,
        });
    }

    match staged_step_after_outer_loop(&run_step, &step_verify_failed_reason) {
        StagedStepAfterOuterLoop::ExecutionOrVerifyFailed { .. } => {
            if staged_step_patch_planner_enabled(patch_ctx.p.ctx.cfg.staged_plan_feedback_mode) {
                let mut recovered = false;
                let patch_budget = staged_patch_budget_after_step_failure(
                    step.max_step_retries,
                    patch_ctx.p.ctx.cfg.staged_plan_patch_max_attempts,
                );
                let audit_footer = patch_ctx
                    .per_coord
                    .staged_plan_patch_vs_plan_rewrite_counters_footer();
                for (attempt_idx, _) in (0..patch_budget).enumerate() {
                    let attempt_1based = attempt_idx.saturating_add(1);
                    let feedback = if let Some(ref vr) = step_verify_failed_reason {
                        let detail_verify = staged_step_verify_fail_patch_detail(vr);
                        let meta = StagedPlanStepFailureFeedbackMeta {
                            plan_id,
                            step_zero_based: i,
                            n_steps_total: n,
                            plan_patch_attempt_one_based: attempt_1based,
                            plan_patch_budget: patch_budget,
                            reason_zh: "本步确定性验证失败 (Step Verification Failed)",
                            detail: detail_verify.as_str(),
                            audit_counters_footer: &audit_footer,
                        };
                        staged_plan_step_failure_feedback_user_body(&meta, &step)
                    } else {
                        let meta = StagedPlanStepFailureFeedbackMeta {
                            plan_id,
                            step_zero_based: i,
                            n_steps_total: n,
                            plan_patch_attempt_one_based: attempt_1based,
                            plan_patch_budget: patch_budget,
                            reason_zh: "执行子循环返回错误",
                            detail: STAGED_STEP_OUTER_LOOP_FAIL_DETAIL,
                            audit_counters_footer: &audit_footer,
                        };
                        staged_plan_step_failure_feedback_user_body(&meta, &step)
                    };
                    if let Some(merged) = run_staged_plan_patch_planner_round(
                        patch_ctx,
                        feedback,
                        plan_steps.as_slice(),
                        i,
                    )
                    .await?
                    {
                        *plan_steps = merged;
                        n = plan_steps.len();
                        push_patch_replan_assistant_json_and_notice(
                            patch_ctx.p,
                            plan_steps.as_slice(),
                            echo_terminal_staged,
                            completed_steps,
                        )
                        .await?;
                        recovered = true;
                        break;
                    }
                }
                if recovered {
                    return Ok(StagedStepIterationCtl::RetryCurrentStep { n });
                }
            }
            finish_staged_plan_step_failed_and_plan_failed_sse(
                StagedPlanStepFailedExit {
                    out: patch_ctx.p.ctx.out,
                    plan_id,
                    step_id_trim: step.id.trim(),
                    step_index,
                    n,
                    completed_steps_before_this: completed_steps,
                },
                step.executor_kind,
                step_verify_failed_reason.as_deref(),
            )
            .await;

            let reason = {
                let mut s = staged_step_failure_retry_exhausted_message(
                    &run_step,
                    &step_verify_failed_reason,
                );
                s.push_str(
                    &patch_ctx
                        .per_coord
                        .staged_plan_patch_vs_plan_rewrite_counters_footer(),
                );
                s
            };
            return Err(RunAgentTurnError::StepRetryExhausted {
                phase: AgentTurnSubPhase::Executor,
                message: reason,
            });
        }
        StagedStepAfterOuterLoop::ProceedToToolCheck => {}
    }

    if sse_sender_closed(patch_ctx.p.ctx.out)
        || patch_ctx
            .p
            .ctx
            .cancel
            .is_some_and(|c| c.load(Ordering::SeqCst))
    {
        finish_staged_plan_step_sse(
            patch_ctx.p.ctx.out,
            plan_id,
            step.id.trim(),
            step_index,
            n,
            "cancelled",
            step.executor_kind,
            None,
        )
        .await;
        return Ok(StagedStepIterationCtl::CancelledAfterOuterOk);
    }

    let tools_ok = staged_step_tool_messages_all_ok(patch_ctx.p.turn.messages, step_user_idx);
    let patch_planner_on =
        staged_step_patch_planner_enabled(patch_ctx.p.ctx.cfg.staged_plan_feedback_mode);
    match staged_step_tool_phase_route(tools_ok, patch_planner_on) {
        StagedStepToolPhaseRoute::AttemptToolFailurePatches => {
            let mut recovered = false;
            let tool_patch_budget = staged_patch_budget_tool_messages_not_ok(
                patch_ctx.p.ctx.cfg.staged_plan_patch_max_attempts,
            );
            let audit_footer = patch_ctx
                .per_coord
                .staged_plan_patch_vs_plan_rewrite_counters_footer();
            for (attempt_idx, _) in (0..tool_patch_budget).enumerate() {
                let attempt_1based = attempt_idx.saturating_add(1);
                let meta = StagedPlanStepFailureFeedbackMeta {
                    plan_id,
                    step_zero_based: i,
                    n_steps_total: n,
                    plan_patch_attempt_one_based: attempt_1based,
                    plan_patch_budget: tool_patch_budget,
                    reason_zh: "本步内工具调用未全部成功",
                    detail: STAGED_STEP_TOOL_MSG_FAIL_DETAIL,
                    audit_counters_footer: &audit_footer,
                };
                let feedback = staged_plan_step_failure_feedback_user_body(&meta, &step);
                if let Some(merged) = run_staged_plan_patch_planner_round(
                    patch_ctx,
                    feedback,
                    plan_steps.as_slice(),
                    i,
                )
                .await?
                {
                    *plan_steps = merged;
                    n = plan_steps.len();
                    push_patch_replan_assistant_json_and_notice(
                        patch_ctx.p,
                        plan_steps.as_slice(),
                        echo_terminal_staged,
                        completed_steps,
                    )
                    .await?;
                    recovered = true;
                    break;
                }
            }
            if recovered {
                return Ok(StagedStepIterationCtl::RetryCurrentStep { n });
            }
            finish_staged_plan_step_failed_and_plan_failed_sse(
                StagedPlanStepFailedExit {
                    out: patch_ctx.p.ctx.out,
                    plan_id,
                    step_id_trim: step.id.trim(),
                    step_index,
                    n,
                    completed_steps_before_this: completed_steps,
                },
                step.executor_kind,
                None,
            )
            .await;
            return Err(RunAgentTurnError::StepRetryExhausted {
                phase: AgentTurnSubPhase::Executor,
                message: format!(
                    "局部修复耗尽上限 (工具执行失败){}",
                    patch_ctx
                        .per_coord
                        .staged_plan_patch_vs_plan_rewrite_counters_footer()
                ),
            });
        }
        StagedStepToolPhaseRoute::EmitStepSuccess => {}
    }

    finish_staged_plan_step_sse(
        patch_ctx.p.ctx.out,
        plan_id,
        step.id.trim(),
        step_index,
        n,
        "ok",
        step.executor_kind,
        None,
    )
    .await;
    patch_ctx
        .p
        .turn
        .messages
        .push(Message::chat_ui_separator(true));
    let plan_row = AgentReplyPlanV1 {
        plan_type: "agent_reply_plan".to_string(),
        version: 1,
        steps: plan_steps.clone(),
        no_task: false,
    };
    send_staged_plan_notice(
        patch_ctx.p.ctx.out,
        echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&plan_row, step_index),
    )
    .await;
    emit_chat_ui_separator_sse(patch_ctx.p.ctx.out, true).await;
    Ok(StagedStepIterationCtl::AdvanceToNextStep {
        n,
        completed_steps: step_index,
    })
}

struct RunOneStagedPlanStepIterationParams<'a, 'b, 'c, F> {
    plan_id: &'a str,
    i: usize,
    n: usize,
    completed_steps: usize,
    plan_steps: &'a mut Vec<PlanStepV1>,
    original_steps: &'a [PlanStepV1],
    transition_counters: &'a mut HashMap<String, u32>,
    echo_terminal_staged: bool,
    labels: &'a StagedPlanRunLabels,
    patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
    make_step_user_message: &'a F,
}

async fn run_one_staged_plan_step_iteration<F>(
    p: RunOneStagedPlanStepIterationParams<'_, '_, '_, F>,
) -> Result<StagedStepIterationCtl, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let RunOneStagedPlanStepIterationParams {
        plan_id,
        i,
        n,
        completed_steps,
        plan_steps,
        original_steps,
        transition_counters,
        echo_terminal_staged,
        labels,
        patch_ctx,
        make_step_user_message,
    } = p;
    let outer = staged_step_run_outer_half(StagedStepRunOuterHalfParams {
        plan_id,
        i,
        n,
        plan_steps: plan_steps.as_slice(),
        echo_terminal_staged,
        labels,
        patch_ctx,
        make_step_user_message,
    })
    .await;

    staged_step_run_after_outer_half(StagedStepRunAfterOuterHalfParams {
        outer,
        plan_id,
        i,
        n,
        completed_steps,
        plan_steps,
        original_steps,
        transition_counters,
        echo_terminal_staged,
        patch_ctx,
    })
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_staged_plan_steps_loop<F>(
    plan_id: String,
    mut plan_steps: Vec<PlanStepV1>,
    original_steps: Vec<PlanStepV1>,
    echo_terminal_staged: bool,
    labels: &StagedPlanRunLabels,
    mut patch_ctx: StagedPlanPatchPlannerCtx<'_, '_, F>,
    make_step_user_message: &F,
) -> Result<StagedPlanRunOutcome, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let mut n = plan_steps.len();
    let orch_phase = staged_orchestrator::enter_steps_executing(
        patch_ctx.p.ctx.out,
        plan_id.as_str(),
        echo_terminal_staged,
        plan_steps.as_slice(),
    )
    .await;
    tracing::info!(
        target: "crabmate::staged",
        staged_fsm = "steps_loop",
        steps_loop_phase = "steps_executing_enter",
        staged_round_orchestrator_phase = ?orch_phase,
        plan_id = plan_id.as_str(),
        step_count = n,
        sub_phase = "executor",
        "staged plan steps loop: started SSE + queue notice"
    );

    let mut staged_loop_cancelled = false;
    let mut completed_steps = 0usize;
    let mut i = 0usize;
    let mut transition_counters: HashMap<String, u32> = HashMap::new();
    let start_time = std::time::Instant::now();
    while i < plan_steps.len() {
        tracing::debug!(
            target: "crabmate::staged",
            staged_fsm = "steps_loop",
            steps_loop_phase = "step_running",
            plan_id = plan_id.as_str(),
            step_index = i,
            step_count = n,
            completed_steps,
            sub_phase = "executor",
            "staged plan steps loop iteration enter"
        );
        if staged_step_wall_clock_exceeded(
            patch_ctx.p.ctx.cfg.max_turn_duration_seconds,
            start_time.elapsed().as_secs(),
        ) {
            return Err(RunAgentTurnError::TimeLimitExhausted {
                phase: AgentTurnSubPhase::Executor,
                message: crate::agent::turn_budget::turn_wall_clock_limit_user_message(
                    patch_ctx.p.ctx.cfg.max_turn_duration_seconds,
                ),
            });
        }

        if sse_sender_closed(patch_ctx.p.ctx.out)
            || patch_ctx
                .p
                .ctx
                .cancel
                .is_some_and(|c| c.load(Ordering::SeqCst))
        {
            staged_loop_cancelled = true;
            tracing::info!(
                target: "crabmate::staged",
                staged_fsm = "steps_loop",
                steps_loop_phase = "cancelled_before_step",
                plan_id = plan_id.as_str(),
                step_index = i,
                step_count = n,
                completed_steps,
                sub_phase = "executor",
                "staged plan steps loop: SSE closed or user cancel"
            );
            break;
        }

        match run_one_staged_plan_step_iteration(RunOneStagedPlanStepIterationParams {
            plan_id: plan_id.as_str(),
            i,
            n,
            completed_steps,
            plan_steps: &mut plan_steps,
            original_steps: original_steps.as_slice(),
            transition_counters: &mut transition_counters,
            echo_terminal_staged,
            labels,
            patch_ctx: &mut patch_ctx,
            make_step_user_message,
        })
        .await?
        {
            StagedStepIterationCtl::RetryCurrentStep { n: new_n } => {
                n = new_n;
            }
            StagedStepIterationCtl::AdvanceToNextStep {
                n: new_n,
                completed_steps: new_completed,
            } => {
                n = new_n;
                completed_steps = new_completed;
                i += 1;
            }
            StagedStepIterationCtl::CancelledAfterOuterOk => {
                staged_loop_cancelled = true;
                tracing::info!(
                    target: "crabmate::staged",
                    staged_fsm = "steps_loop",
                    steps_loop_phase = "cancelled_after_outer_ok",
                    plan_id = plan_id.as_str(),
                    step_index = i,
                    step_count = n,
                    completed_steps,
                    sub_phase = "executor",
                    "staged plan steps loop: cancelled after outer_loop ok"
                );
                break;
            }
        }
    }
    tracing::info!(
        target: "crabmate::staged",
        staged_fsm = "steps_loop",
        steps_loop_phase = "send_plan_finished",
        plan_id = plan_id.as_str(),
        step_count = n,
        completed_steps,
        finish_status = if staged_loop_cancelled {
            "cancelled"
        } else {
            "ok"
        },
        sub_phase = "executor",
        "staged plan steps loop: emitting staged_plan_finished"
    );
    // 末步成功后循环内已发送含「[✓] 全部完成」的摘要，勿再发一次（否则重复一条）。
    send_staged_plan_finished(
        patch_ctx.p.ctx.out,
        &plan_id,
        n,
        completed_steps,
        if staged_loop_cancelled {
            "cancelled"
        } else {
            "ok"
        },
    )
    .await;
    // 仅当循环内未添加过分隔符时再追加：n==0 未进入循环；或取消时 completed_steps==0。
    // 否则末步成功后已在循环内添加，再加会重复两行。
    if n == 0 || (staged_loop_cancelled && completed_steps == 0) {
        patch_ctx
            .p
            .turn
            .messages
            .push(Message::chat_ui_separator(true));
        emit_chat_ui_separator_sse(patch_ctx.p.ctx.out, true).await;
    }
    Ok(StagedPlanRunOutcome::ContinuePlanning)
}

/// 首轮解析成功后 **`PreparedPlannerRoute::ContinueWithPlan`** 的后续管线（no_task / full-pipeline）参聚合。
struct ContinuePreparedPlanAfterFirstRoundParams<'a, 'b, F> {
    p: &'a mut RunLoopParams<'b>,
    per_coord: &'a mut PerCoordinator,
    labels: StagedPlanRunLabels,
    planner_render_to_terminal: bool,
    echo_terminal_staged: bool,
    plan: plan_artifact::AgentReplyPlanV1,
    msg: Message,
    make_step_user_message: F,
}

async fn continue_prepared_plan_after_first_round<F>(
    params: ContinuePreparedPlanAfterFirstRoundParams<'_, '_, F>,
) -> Result<StagedPlanRunOutcome, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let ContinuePreparedPlanAfterFirstRoundParams {
        p,
        per_coord,
        labels,
        planner_render_to_terminal,
        echo_terminal_staged,
        plan,
        msg,
        make_step_user_message,
    } = params;
    let omit_no_task_planner_from_history = omit_no_task_planner_from_history(
        p.ctx.out.is_some(),
        crate::web::web_ui_env::web_raw_assistant_output_env(),
        plan.no_task,
    );
    if !omit_no_task_planner_from_history {
        push_assistant_merging_trailing_empty_placeholder(p.turn.messages, msg.clone());
    }

    let post_schedule = prepared_post_parse_schedule(plan.no_task);
    tracing::debug!(
        target: "crabmate::staged",
        staged_fsm = "prepared_request",
        prepared_route = "continue_with_plan",
        post_parse_schedule = ?post_schedule,
        plan_no_task = plan.no_task,
        plan_steps_len = plan.steps.len(),
        sub_phase = "planner",
        "staged prepared_request continue: post-parse schedule"
    );

    match post_schedule {
        PreparedPostParseSchedule::NoTaskThenOuter => {
            run_no_task_branch_then_outer(p, per_coord, &make_step_user_message).await?;
            Ok(StagedPlanRunOutcome::Finished)
        }
        PreparedPostParseSchedule::FullPipelineThenSteps => {
            let parallel_csv = plan_optimizer::parallel_batchable_tool_names_csv_from_defs(
                p.ctx.tools_defs,
                p.ctx.cfg.as_ref(),
            );
            let validate_only_binding_active =
                plan_rewrite::last_workflow_validate_binding_plan_node_ids(p.turn.messages)
                    .is_some_and(|ids| !ids.is_empty());
            let trigger_user = plan_optimizer::staged_plan_trigger_user_content(p.turn.messages);
            let pipeline_schedule = prepared_full_pipeline_schedule(PreparedFullPipelineInputs {
                staged_plan_ensemble_count: p.ctx.staged_plan_ensemble_count,
                staged_plan_skip_ensemble_on_casual_prompt: p
                    .ctx
                    .staged_plan_skip_ensemble_on_casual_prompt,
                validate_only_binding_active,
                trigger_user_content: trigger_user,
                plan_steps_len: plan.steps.len(),
                staged_plan_optimizer_round: p.ctx.staged_plan_optimizer_round,
                staged_plan_optimizer_requires_parallel_tools: p
                    .ctx
                    .staged_plan_optimizer_requires_parallel_tools,
                parallel_tool_names_csv: parallel_csv.as_str(),
                staged_plan_two_phase_nl_display: p.ctx.cfg.staged_plan_two_phase_nl_display,
            });

            advance_full_pipeline_phases_after_parse_inner(AdvanceFullPipelineAfterParseParams {
                p,
                per_coord,
                labels,
                planner_render_to_terminal,
                echo_terminal_staged,
                make_step_user_message: &make_step_user_message,
                plan,
                pipeline_schedule,
                parallel_csv,
            })
            .await
        }
    }
}

/// 首轮无工具规划轮：可选 tool_calls 重写后再返回 assistant。
async fn complete_first_planner_round_maybe_retry_tool_reject<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    req: &crate::types::ChatRequest,
    planner_render_to_terminal: bool,
    labels: StagedPlanRunLabels,
    make_step_user_message: &F,
) -> Result<(Message, String), RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let (mut first_msg, first_finish) =
        complete_planner_no_tools_chat_retrying(p, req, planner_render_to_terminal).await?;
    let (msg, finish_reason) = if first_finish != USER_CANCELLED_FINISH_REASON {
        let first_total = staged_first_planner_round_tool_call_total_after_materialize(
            &mut first_msg,
            p.ctx.cfg.materialize_deepseek_dsml_tool_calls,
        );
        if first_total > 0 {
            warn!(
                target: "crabmate",
                "分阶段规划轮：检测到 {} 条 tool_calls，严格无工具模式触发一次轻量重写",
                first_total
            );
            emit_staged_planner_tool_call_rejected_timeline(p.ctx.out, first_total).await;
            p.turn.messages.push(make_step_user_message(
                staged_planner_tool_call_reject_user_body(first_total),
            ));
            let retry_req = prepare_staged_planner_no_tools_request(
                p,
                per_coord,
                labels.build_planner_messages,
            )
            .await?;
            complete_planner_no_tools_chat_retrying(p, &retry_req, planner_render_to_terminal)
                .await?
        } else {
            (first_msg, first_finish)
        }
    } else {
        (first_msg, first_finish)
    };

    Ok((msg, finish_reason))
}

fn debug_first_planner_finish(labels: StagedPlanRunLabels, finish_reason: &str, msg: &Message) {
    debug!(
        target: "crabmate",
        "{} finish_reason={} assistant_preview={}",
        labels.planning_log_label,
        finish_reason,
        crate::redact::assistant_message_preview_for_log(msg)
    );
}

async fn strip_non_tool_planner_assistant_after_first_round(
    msg: &mut Message,
    p: &RunLoopParams<'_>,
) {
    let raw_tool_calls = msg.tool_calls.as_ref().map(|c| c.len()).unwrap_or(0);
    if raw_tool_calls > 0 {
        warn!(
            target: "crabmate",
            "分阶段规划轮重写后仍返回 {} 条原生 tool_calls，严格无工具模式下将其忽略",
            raw_tool_calls
        );
    }
    msg.tool_calls = None;
    crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(
        msg,
        p.ctx.cfg.materialize_deepseek_dsml_tool_calls,
    );
    let dsml_tool_calls = msg.tool_calls.as_ref().map(|c| c.len()).unwrap_or(0);
    if dsml_tool_calls > 0 {
        emit_staged_planner_tool_call_rejected_timeline(p.ctx.out, dsml_tool_calls).await;
        warn!(
            target: "crabmate",
            "分阶段规划轮重写后仍检测到 {} 条 DSML tool_calls；严格无工具模式下将其忽略",
            dsml_tool_calls
        );
    }
    msg.tool_calls = None;
}

async fn run_no_task_branch_then_outer<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    make_step_user_message: &F,
) -> Result<(), RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    if p.ctx.cfg.staged_plan_two_phase_nl_display {
        run_staged_plan_nl_followup_round(p, per_coord, make_step_user_message).await?;
    }
    debug!(
        target: "crabmate",
        "分阶段规划：no_task=true，跳过分步注入，转入常规对话循环"
    );
    run_agent_outer_loop(p, per_coord).await?;
    Ok(())
}

/// 分阶段规划优化轮：入参聚合（控制 `clippy::too_many_arguments`）。
struct StagedOptimizerRoundParams<'a, 'b, F> {
    p: &'a mut RunLoopParams<'b>,
    per_coord: &'a mut PerCoordinator,
    labels: StagedPlanRunLabels,
    planner_render_to_terminal: bool,
    make_step_user_message: &'a F,
    plan: &'a mut plan_artifact::AgentReplyPlanV1,
    optimizer_route: planner_round_fsm::StagedPlanOptimizerRoute,
    parallel_csv: &'a str,
}

async fn maybe_run_optimizer_round_and_apply_steps_inner<F>(
    params: StagedOptimizerRoundParams<'_, '_, F>,
) -> Result<ControlFlow<StagedPlanRunOutcome, ()>, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let StagedOptimizerRoundParams {
        p,
        per_coord,
        labels,
        planner_render_to_terminal,
        make_step_user_message,
        plan,
        optimizer_route,
        parallel_csv,
    } = params;
    if !optimizer_round_should_run(optimizer_route) {
        return Ok(ControlFlow::Continue(()));
    }
    let opt_body = plan_optimizer::staged_plan_optimizer_user_body(plan, parallel_csv);
    p.turn.messages.push(make_step_user_message(opt_body));
    let (mut opt_msg, opt_finish) = complete_one_staged_planner_assistant_round(
        p,
        per_coord,
        labels.build_planner_messages,
        planner_render_to_terminal,
        "分阶段规划优化轮模型输出",
    )
    .await?;
    if opt_finish == USER_CANCELLED_FINISH_REASON {
        pop_last_staged_planner_coach_user_if_present(p.turn.messages);
        return Ok(ControlFlow::Break(StagedPlanRunOutcome::Finished));
    }
    strip_staged_planner_message_tool_calls(
        &mut opt_msg,
        "优化轮",
        p.ctx.cfg.materialize_deepseek_dsml_tool_calls,
    );
    let opt_content = crate::types::message_content_as_str(&opt_msg.content).unwrap_or("");
    let merged_steps = plan_optimizer::try_parse_optimizer_reply(opt_content);
    match ensemble_merge_outcome_from_parsed_steps(merged_steps) {
        EnsembleMergeOutcome::AppliedSteps(steps) => {
            if steps.len() < plan.steps.len() {
                debug!(
                    target: "crabmate",
                    "分阶段规划优化轮：步数 {} -> {}",
                    plan.steps.len(),
                    steps.len()
                );
            }
            push_assistant_merging_trailing_empty_placeholder(p.turn.messages, opt_msg);
            plan.steps = steps;
        }
        EnsembleMergeOutcome::KeepPriorPlan => {
            warn!(
                target: "crabmate",
                "分阶段规划优化轮：未解析出合法 agent_reply_plan v1 或非空 steps，沿用首轮规划"
            );
            pop_last_staged_planner_coach_user_if_present(p.turn.messages);
        }
    }
    Ok(ControlFlow::Continue(()))
}

/// 首轮解析后 full-pipeline 直至分步循环：入参聚合（控制 `clippy::too_many_arguments`）。
struct AdvanceFullPipelineAfterParseParams<'a, 'b, F> {
    p: &'a mut RunLoopParams<'b>,
    per_coord: &'a mut PerCoordinator,
    labels: StagedPlanRunLabels,
    planner_render_to_terminal: bool,
    echo_terminal_staged: bool,
    make_step_user_message: &'a F,
    plan: plan_artifact::AgentReplyPlanV1,
    pipeline_schedule: PreparedFullPipelineSchedule,
    parallel_csv: String,
}

async fn advance_full_pipeline_phases_after_parse_inner<F>(
    params: AdvanceFullPipelineAfterParseParams<'_, '_, F>,
) -> Result<StagedPlanRunOutcome, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let AdvanceFullPipelineAfterParseParams {
        p,
        per_coord,
        labels,
        planner_render_to_terminal,
        echo_terminal_staged,
        make_step_user_message,
        mut plan,
        pipeline_schedule,
        parallel_csv,
    } = params;
    let ensemble_route = pipeline_schedule.ensemble_route;
    log_staged_plan_ensemble_route(ensemble_route, p.ctx.staged_plan_ensemble_count);

    let mut fp_phase = StagedFullPipelinePhase::BeforeEnsemble;
    debug_staged_full_pipeline_enter(fp_phase);

    if ensemble_merge_should_invoke(ensemble_route) {
        let skip_ensemble_for_casual = ensemble_merge_skip_for_casual_prompt(ensemble_route);
        maybe_run_staged_plan_ensemble_then_merge(
            p,
            per_coord,
            &labels,
            make_step_user_message,
            planner_render_to_terminal,
            &mut plan,
            skip_ensemble_for_casual,
        )
        .await?;
    }
    let next_fp = fp_phase
        .advance()
        .expect("full_pipeline: before_ensemble -> after_ensemble");
    debug_staged_full_pipeline_transition(fp_phase, Some(next_fp));
    fp_phase = next_fp;

    let optimizer_route = pipeline_schedule.optimizer_route;
    log_staged_plan_optimizer_route(optimizer_route, plan.steps.len());

    match maybe_run_optimizer_round_and_apply_steps_inner(StagedOptimizerRoundParams {
        p,
        per_coord,
        labels,
        planner_render_to_terminal,
        make_step_user_message,
        plan: &mut plan,
        optimizer_route,
        parallel_csv: parallel_csv.as_str(),
    })
    .await?
    {
        ControlFlow::Break(outcome) => return Ok(outcome),
        ControlFlow::Continue(()) => {}
    }

    let next_fp = fp_phase
        .advance()
        .expect("full_pipeline: after_ensemble -> after_optimizer");
    debug_staged_full_pipeline_transition(fp_phase, Some(next_fp));
    fp_phase = next_fp;

    if pipeline_schedule.nl_followup_before_steps {
        run_staged_plan_nl_followup_round(p, per_coord, make_step_user_message).await?;
    }
    let next_fp = fp_phase
        .advance()
        .expect("full_pipeline: after_optimizer -> after_nl_followup");
    debug_staged_full_pipeline_transition(fp_phase, Some(next_fp));
    fp_phase = next_fp;

    debug_staged_full_pipeline_transition(fp_phase, None);

    let plan_id = next_staged_plan_id();
    let plan_steps = plan.steps;
    let original_steps = plan_steps.clone();
    let patch_ctx = StagedPlanPatchPlannerCtx {
        p,
        per_coord,
        labels: &labels,
        planner_render_to_terminal,
        make_step_user_message,
    };

    run_staged_plan_steps_loop(
        plan_id,
        plan_steps,
        original_steps,
        echo_terminal_staged,
        &labels,
        patch_ctx,
        make_step_user_message,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_staged_plan_with_prepared_request<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    req: crate::types::ChatRequest,
    render_to_terminal: bool,
    echo_terminal_staged: bool,
    entered_from_step_execution_round: bool,
    labels: StagedPlanRunLabels,
    make_step_user_message: F,
) -> Result<StagedPlanRunOutcome, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let planner_render_to_terminal = render_to_terminal
        && (p.ctx.out.is_some() || p.ctx.cfg.staged_plan_cli_show_planner_stream);
    let (mut msg, finish_reason) = complete_first_planner_round_maybe_retry_tool_reject(
        p,
        per_coord,
        &req,
        planner_render_to_terminal,
        labels,
        &make_step_user_message,
    )
    .await?;

    debug_first_planner_finish(labels, finish_reason.as_str(), &msg);

    if finish_reason == USER_CANCELLED_FINISH_REASON {
        return Ok(StagedPlanRunOutcome::Finished);
    }

    strip_non_tool_planner_assistant_after_first_round(&mut msg, p).await;

    let merged_for_log =
        crate::agent::plan_artifact::assistant_merged_text_for_plan_artifact_parse(&msg);
    let validate_only_binding_ids =
        plan_rewrite::last_workflow_validate_binding_plan_node_ids(p.turn.messages);
    let parse_result =
        crate::agent::plan_artifact::parse_agent_reply_plan_v1_from_assistant_message_with_validate_only_binding_ids(
            &msg,
            validate_only_binding_ids.as_deref(),
        );
    let parse_err_detail = parse_result
        .as_ref()
        .err()
        .map(crate::agent::plan_artifact::plan_artifact_error_log_summary);
    let degrade_like_not_found = matches!(
        parse_result.as_ref().err(),
        Some(crate::agent::plan_artifact::PlanArtifactError::NotFound)
    );

    let route = resolve_prepared_planner_route(
        parse_result,
        entered_from_step_execution_round,
        &msg,
        merged_for_log,
        parse_err_detail,
        degrade_like_not_found,
    );
    tracing::debug!(
        target: "crabmate::staged",
        staged_fsm = "prepared_request",
        prepared_route = route.as_static_str(),
        entered_from_step_execution_round,
        sub_phase = "planner",
        "staged prepared_request first-round parse route"
    );

    match route {
        PreparedPlannerRoute::QuietFinish => {
            debug!(
                target: "crabmate",
                "分阶段重规划：检测到分步执行后重入且本轮未产出结构化计划，视为收敛完成，直接结束（避免重复总结）"
            );
            Ok(StagedPlanRunOutcome::Finished)
        }
        PreparedPlannerRoute::DegradeToOuterLoop => {
            push_assistant_merging_trailing_empty_placeholder(p.turn.messages, msg.clone());
            run_agent_outer_loop(p, per_coord).await?;
            Ok(StagedPlanRunOutcome::Finished)
        }
        PreparedPlannerRoute::ContinueWithPlan { plan } => {
            continue_prepared_plan_after_first_round(ContinuePreparedPlanAfterFirstRoundParams {
                p,
                per_coord,
                labels,
                planner_render_to_terminal,
                echo_terminal_staged,
                plan,
                msg,
                make_step_user_message,
            })
            .await
        }
    }
}

pub(super) async fn run_logical_dual_agent_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    let render_to_terminal = p.ctx.render_to_terminal;
    let echo_terminal_staged = render_to_terminal && p.ctx.out.is_none();

    let labels = StagedPlanRunLabels {
        planning_log_label: "逻辑双agent规划轮输出",
        step_injection_log_label: "逻辑双agent注入执行器user",
        build_planner_messages: build_logical_dual_planner_messages,
    };

    run_staged_rolling_horizon_outer_loop(
        StagedRollingHorizonKind::LogicalDualAgent,
        p,
        per_coord,
        labels,
        render_to_terminal,
        echo_terminal_staged,
        Message::user_only,
    )
    .await
}

#[cfg(test)]
mod staged_not_found_convergence_tests {
    use crate::agent::plan_artifact::PlanArtifactError;

    use super::planner_parse_fsm::{
        StagedPlannerParseRoute, entered_implies_finish_on_plan_not_found,
        staged_planner_parse_route,
    };

    #[test]
    fn not_found_does_not_finish_for_plain_qa_round() {
        assert!(
            !entered_implies_finish_on_plan_not_found(false),
            "普通问答轮（未进入步后重规划）遇到 NotFound 不应直接收敛结束"
        );
        assert!(
            !matches!(
                staged_planner_parse_route(&PlanArtifactError::NotFound, false),
                StagedPlannerParseRoute::QuietFinishOnPlanNotFound
            ),
            "路由应与 entered 标记一致"
        );
    }

    #[test]
    fn not_found_finishes_only_after_step_execution_reentry() {
        assert!(
            entered_implies_finish_on_plan_not_found(true),
            "仅在同 turn 的步后重规划轮，NotFound 才应触发收敛结束"
        );
        assert!(
            matches!(
                staged_planner_parse_route(&PlanArtifactError::NotFound, true),
                StagedPlannerParseRoute::QuietFinishOnPlanNotFound
            ),
            "路由应与 entered 标记一致"
        );
    }
}

/// `prepare_messages_for_model` 与规划轮请求拼装组合的回归护栏（不经真实 HTTP）。
#[cfg(test)]
mod staged_plan_prepare_fixture_tests {
    use std::sync::Arc;

    use crate::agent::context_window::prepare_messages_for_model;
    use crate::agent::per_coord::{PerCoordinator, PerCoordinatorInit};
    use crate::llm::OPENAI_COMPAT_BACKEND;
    use crate::types::{LlmSeedOverride, Message, message_content_as_str};

    use super::super::errors::AgentTurnSubPhase;
    use super::super::params::{RunLoopCtx, RunLoopParams, RunLoopTurnState};
    use super::staged_sse::staged_plan_phase_instruction_default;
    use super::{build_single_agent_planner_messages, prepare_staged_planner_no_tools_request};

    #[tokio::test]
    async fn prepare_then_build_planner_messages_ends_with_plan_system() {
        let cfg = Arc::new(crate::config::load_config(None).expect("embed default"));
        let client = reqwest::Client::new();
        let mut messages = vec![
            Message::user_only("请在本仓库执行一次 cargo check 并汇报结果"),
            Message::assistant_only(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"运行 cargo check"}]}
```"#,
            ),
        ];
        let mut per = PerCoordinator::new(PerCoordinatorInit::from_agent_config(cfg.as_ref()));
        prepare_messages_for_model(
            &OPENAI_COMPAT_BACKEND,
            &client,
            "",
            cfg.as_ref(),
            &mut messages,
            Some(&mut per),
            None,
        )
        .await
        .expect("prepare_messages_for_model");

        let plan_sys = staged_plan_phase_instruction_default();
        let preserve_kimi = crate::llm::llm_vendor_adapter(cfg.as_ref())
            .preserve_assistant_tool_call_reasoning(cfg.as_ref());
        let preserve_deepseek = crate::llm::vendor::deepseek_json_output_eligible(cfg.as_ref());
        let built = build_single_agent_planner_messages(
            messages.as_slice(),
            plan_sys.clone(),
            preserve_kimi,
            preserve_deepseek,
        );
        let last = built.last().expect("non-empty planner messages");
        assert_eq!(last.role, "system");
        let body = message_content_as_str(&last.content).unwrap_or("");
        assert!(
            body.contains("agent_reply_plan"),
            "规划 system 应包含 schema 约定片段"
        );
        assert!(
            body.len() >= plan_sys.len().saturating_sub(40),
            "system 正文应接近完整规划轮指令"
        );
    }

    #[tokio::test]
    async fn prepare_staged_planner_no_tools_request_fixture_roundtrip() {
        let cfg = Arc::new(crate::config::load_config(None).expect("embed default"));
        let client = reqwest::Client::new();
        let mut messages = vec![Message::user_only("fixture：分阶段规划请求拼装")];
        let mut per = PerCoordinator::new(PerCoordinatorInit::from_agent_config(cfg.as_ref()));

        let mut p = RunLoopParams {
            ctx: RunLoopCtx {
                llm_backend: &OPENAI_COMPAT_BACKEND,
                client: &client,
                api_key: "",
                cfg: &cfg,
                tools_defs: &[],
                out: None,
                effective_working_dir: std::path::Path::new("."),
                workspace_is_set: false,
                no_stream: true,
                cancel: None,
                render_to_terminal: false,
                plain_terminal_stream: false,
                web_tool_ctx: None,
                cli_tool_ctx: None,
                per_flight: None,
                long_term_memory: None,
                long_term_memory_scope_id: None,
                mcp_session: None,
                read_file_turn_cache: None,
                workspace_changelist: None,
                staged_plan_optimizer_round: cfg.staged_plan_optimizer_round,
                staged_plan_optimizer_requires_parallel_tools: cfg
                    .staged_plan_optimizer_requires_parallel_tools,
                staged_plan_ensemble_count: cfg.staged_plan_ensemble_count,
                staged_plan_skip_ensemble_on_casual_prompt: cfg
                    .staged_plan_skip_ensemble_on_casual_prompt,
                request_chrome_trace: None,
                turn_allowed_tool_names: None,
                tracing_chat_turn: None,
            },
            turn: RunLoopTurnState {
                messages: &mut messages,
                sub_phase: AgentTurnSubPhase::Planner,
                intent_turn_gate_hint: None,
                step_executor_constraint: None,
                temperature_override: None,
                model_override: None,
                use_executor_model: false,
                executor_model_override: None,
                executor_api_base: None,
                executor_api_key: None,
                seed_override: LlmSeedOverride::FromConfig,
            },
        };

        let req = prepare_staged_planner_no_tools_request(
            &mut p,
            &mut per,
            build_single_agent_planner_messages,
        )
        .await
        .expect("prepare_staged_planner_no_tools_request");

        assert!(
            req.messages.iter().any(|m| {
                message_content_as_str(&m.content)
                    .is_some_and(|c| c.contains("fixture：分阶段规划请求拼装"))
            }),
            "用户正文应在上下文变换后仍出现在 ChatRequest.messages"
        );
        assert!(
            req.messages.iter().any(|m| {
                m.role == "system"
                    && message_content_as_str(&m.content).is_some_and(|c| c.contains("分阶段规划"))
            }),
            "末尾规划 system 应进入 ChatRequest"
        );
    }
}
