//! 分阶段规划与逻辑双 agent：规划轮 + 逐步注入执行。

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use log::{debug, warn};
use tokio::sync::mpsc;

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::{self, AgentReplyPlanV1, PlanStepV1};
use crate::agent::plan_ensemble;
use crate::agent::plan_optimizer::{self, STAGED_PLAN_OPTIMIZER_COACH_MARK};
use crate::agent::reflection::plan_rewrite;
use crate::config::StagedPlanFeedbackMode;
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
mod orchestrator;
mod patch_planner;
mod planner_round_fsm;
mod sse;
mod turn_fsm;

use orchestrator as staged_orchestrator;
use sse as staged_sse;

use ensemble_fsm::{
    EnsembleMergeOutcome, EnsembleSecondaryPlannerRoundOutcome,
    ensemble_merge_outcome_from_parsed_steps, ensemble_secondary_planner_round_outcome,
};
use patch_planner::{
    StagedPlanPatchPlannerCtx, run_staged_plan_patch_planner_round,
    staged_plan_step_failure_feedback_user_body,
};
use planner_round_fsm::{
    StagedPlanEnsembleRoute, StagedPlanOptimizerRoute, staged_plan_ensemble_route,
    staged_plan_optimizer_route,
};
use staged_sse::{
    emit_chat_ui_separator_sse, next_staged_plan_id, send_staged_plan_finished,
    send_staged_plan_notice, send_staged_plan_step_finished, send_staged_plan_step_started,
    staged_plan_nl_followup_user_body, staged_plan_phase_instruction_default,
    staged_plan_queue_summary_text,
};
use turn_fsm::{
    StagedTurnAdvance, StagedTurnPhase, StagedTurnSubCallOutcome,
    advance_staged_turn_after_sub_call, entered_flag_for_next_planner_call,
    next_rewrite_attempts_after_advance,
};

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
    let extra = p.ctx.staged_plan_ensemble_count.saturating_sub(1);
    if extra == 0 {
        return Ok(());
    }
    if skip_for_casual_user_prompt {
        debug!(
            target: "crabmate",
            "分阶段规划·逻辑多规划员：用户输入偏短/寒暄启发式，跳过 ensemble（staged_plan_ensemble_count={}）以省 API",
            p.ctx.staged_plan_ensemble_count
        );
        return Ok(());
    }

    let dsml = p.ctx.cfg.materialize_deepseek_dsml_tool_calls;
    let mut accepted: Vec<AgentReplyPlanV1> = vec![plan.clone()];

    for i in 0..extra {
        let planner_idx = i.saturating_add(2);
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

    if accepted.len() < 2 {
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
            return Err(RunAgentTurnError::Other {
                phase: AgentTurnSubPhase::Planner,
                message: format!(
                    "分阶段单步规划轮次超过上限（{}），已停止以避免无限循环",
                    STAGED_SINGLE_STEP_MAX_ROUNDS
                ),
            });
        }
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
        .await;

        let event = match res {
            Ok(o) => StagedTurnSubCallOutcome::Ok(o),
            Err(e) => StagedTurnSubCallOutcome::Err(e),
        };
        let advance =
            advance_staged_turn_after_sub_call(phase, rewrite_attempts, max_rewrites, event);
        rewrite_attempts = next_rewrite_attempts_after_advance(rewrite_attempts, &advance);

        match advance {
            StagedTurnAdvance::Continue {
                phase: next_phase,
                push_feedback_user,
            } => {
                phase = next_phase;
                if let Some(u) = push_feedback_user {
                    if let Some(ref snap) = snapshot {
                        if let Err(e) = snap.restore() {
                            tracing::warn!(target: "crabmate", "工作区快照回滚失败: {}", e);
                        } else {
                            tracing::info!(target: "crabmate", "全局重规划触发，工作区已回滚到快照状态");
                        }
                    }
                    p.turn.messages.push(u);
                } else if matches!(phase, StagedTurnPhase::AfterStepExecutionRound) {
                    debug!(
                        target: "crabmate",
                        "分阶段单步：本轮执行完成，进入下一轮无工具规划（round={}）",
                        staged_rounds
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

#[inline]
fn should_finish_when_plan_not_found(entered_from_step_execution_round: bool) -> bool {
    entered_from_step_execution_round
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

fn compute_transition_trigger(
    step: &PlanStepV1,
    run_failed_or_verify_failed: bool,
    step_verify_failed_reason: &Option<String>,
    transition_counters: &mut HashMap<String, u32>,
) -> Option<(String, String)> {
    let transitions = step.transitions.as_ref()?;
    let target = if run_failed_or_verify_failed {
        transitions
            .iter()
            .find(|t| t.condition == "on_verify_fail" || t.condition == "always")
    } else {
        transitions
            .iter()
            .find(|t| t.condition == "on_verify_success" || t.condition == "always")
    }?;
    let key = format!("{}->{}", step.id, target.target_step_id);
    let count = transition_counters.entry(key).or_insert(0);
    if *count >= target.max_loops.unwrap_or(3) {
        return None;
    }
    *count += 1;
    let reason = if run_failed_or_verify_failed {
        step_verify_failed_reason
            .clone()
            .unwrap_or_else(|| "执行错误".to_string())
    } else {
        "执行成功".to_string()
    };
    Some((target.target_step_id.clone(), reason))
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
    staged_orchestrator::enter_steps_executing(
        patch_ctx.p.ctx.out,
        plan_id.as_str(),
        echo_terminal_staged,
        plan_steps.as_slice(),
    )
    .await;

    let mut staged_loop_cancelled = false;
    let mut completed_steps = 0usize;
    let mut i = 0usize;
    let mut transition_counters: HashMap<String, u32> = HashMap::new();
    let start_time = std::time::Instant::now();
    while i < plan_steps.len() {
        if patch_ctx.p.ctx.cfg.max_turn_duration_seconds > 0
            && start_time.elapsed().as_secs() > patch_ctx.p.ctx.cfg.max_turn_duration_seconds
        {
            return Err(RunAgentTurnError::TimeLimitExhausted {
                phase: AgentTurnSubPhase::Executor,
                message: format!(
                    "已达到单轮墙钟时间上限 ({}秒)",
                    patch_ctx.p.ctx.cfg.max_turn_duration_seconds
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
            break;
        }
        let step = plan_steps[i].clone();
        let step_index = i + 1;
        send_staged_plan_step_started(
            patch_ctx.p.ctx.out,
            &plan_id,
            step.id.trim(),
            step_index,
            n,
            step.description.trim(),
            step.executor_kind.map(|k| k.as_snake_case_str()),
        )
        .await;

        let summary_hint = if step_index == n && n > 1 {
            format!(
                "\n本步为最后一步，终答中请简要列出本轮全部 {} 个步骤的完成情况（可对每步附简短说明）。",
                n
            )
        } else {
            String::new()
        };
        let sub_agent_hint = match step.executor_kind {
            Some(crate::agent::plan_artifact::PlanStepExecutorKind::ReviewReadonly) => {
                "\n- **子代理角色**：`review_readonly`（本步仅允许只读类工具；禁止 MCP 与写盘）\n"
            }
            Some(crate::agent::plan_artifact::PlanStepExecutorKind::PatchWrite) => {
                "\n- **子代理角色**：`patch_write`（本步仅允许只读工具与受限补丁写：`apply_patch` / `search_replace` / `structured_patch` / `create_file` / `modify_file` / `append_file` / `format_file` / `ast_grep_rewrite`）\n"
            }
            Some(crate::agent::plan_artifact::PlanStepExecutorKind::TestRunner) => {
                "\n- **子代理角色**：`test_runner`（本步允许只读工具、内置测试运行器如 `cargo_test` / `pytest_run` / `go_test`，以及 **`run_command`** 执行配置白名单内的编译/检查命令，例如 `cargo build`、`cargo check`）\n"
            }
            None => "",
        };
        let body = format!(
            "### 分步 {}/{}\n{}{}{}\n- id: {}\n- 描述: {}",
            step_index,
            n,
            crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE,
            summary_hint,
            sub_agent_hint,
            step.id,
            step.description
        );
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

        let transition_triggered = compute_transition_trigger(
            &step,
            run_step.is_err() || step_verify_failed_reason.is_some(),
            &step_verify_failed_reason,
            &mut transition_counters,
        );

        if let Some((target_id, reason)) = transition_triggered {
            let target_idx_opt = original_steps.iter().position(|s| s.id == target_id);
            if let Some(target_idx) = target_idx_opt {
                let mut new_suffix = original_steps[target_idx..].to_vec();
                let loop_suffix = format!("-loop{}", i);
                for s in &mut new_suffix {
                    s.id = format!("{}{}", s.id, loop_suffix);
                }

                plan_steps.truncate(i + 1);
                plan_steps.extend(new_suffix);
                n = plan_steps.len();

                let fb = format!(
                    "### 状态机流转：触发控制流跳转\n\
                     根据规划设定的 transitions 规则，由于 [{}]，系统已追加回退或跳转到步骤 `{}` 的执行指令。\n\
                     请注意调整接下来的工具调用。",
                    reason, target_id
                );
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

                let step_status = if run_step.is_err() || step_verify_failed_reason.is_some() {
                    "failed"
                } else {
                    "ok"
                };
                let step_verify_fail_reason = step_verify_failed_reason.as_deref();
                finish_staged_plan_step_sse(
                    patch_ctx.p.ctx.out,
                    &plan_id,
                    step.id.trim(),
                    step_index,
                    n,
                    step_status,
                    step.executor_kind,
                    step_verify_fail_reason,
                )
                .await;
                completed_steps = step_index;
                patch_ctx
                    .p
                    .turn
                    .messages
                    .push(Message::chat_ui_separator(true));
                emit_chat_ui_separator_sse(patch_ctx.p.ctx.out, true).await;
                i += 1;
                continue;
            }
        }

        if run_step.is_err() || step_verify_failed_reason.is_some() {
            if patch_ctx.p.ctx.cfg.staged_plan_feedback_mode == StagedPlanFeedbackMode::PatchPlanner
            {
                let mut recovered = false;
                let max_retries = step
                    .max_step_retries
                    .unwrap_or(patch_ctx.p.ctx.cfg.staged_plan_patch_max_attempts as u32)
                    as usize;
                for _ in 0..max_retries {
                    let feedback = if let Some(ref vr) = step_verify_failed_reason {
                        staged_plan_step_failure_feedback_user_body(
                            &plan_id,
                            i,
                            n,
                            &step,
                            "本步确定性验证失败 (Step Verification Failed)",
                            &format!(
                                "验证闸门报告失败: {}\n请根据对话历史缩短或调整后续步骤，并在补丁中修复此问题。",
                                vr
                            ),
                        )
                    } else {
                        staged_plan_step_failure_feedback_user_body(
                            &plan_id,
                            i,
                            n,
                            &step,
                            "执行子循环返回错误",
                            "请根据对话历史缩短或调整后续步骤；若属环境/权限问题请在补丁中显式增加修复步。",
                        )
                    };
                    if let Some(merged) = run_staged_plan_patch_planner_round(
                        &mut patch_ctx,
                        feedback,
                        &plan_steps,
                        i,
                    )
                    .await?
                    {
                        plan_steps = merged;
                        n = plan_steps.len();
                        let replan = AgentReplyPlanV1 {
                            plan_type: "agent_reply_plan".to_string(),
                            version: 1,
                            steps: plan_steps.clone(),
                            no_task: false,
                        };
                        let json = plan_artifact::agent_reply_plan_v1_to_json_string(&replan)
                            .map_err(|e| RunAgentTurnError::Other {
                                phase: AgentTurnSubPhase::Executor,
                                message: e.to_string(),
                            })?;
                        push_assistant_merging_trailing_empty_placeholder(
                            patch_ctx.p.turn.messages,
                            Message::assistant_only(json),
                        );
                        send_staged_plan_notice(
                            patch_ctx.p.ctx.out,
                            echo_terminal_staged,
                            true,
                            staged_plan_queue_summary_text(&replan, completed_steps),
                        )
                        .await;
                        recovered = true;
                        break;
                    }
                }
                if recovered {
                    continue;
                }
            }
            finish_staged_plan_step_failed_and_plan_failed_sse(
                StagedPlanStepFailedExit {
                    out: patch_ctx.p.ctx.out,
                    plan_id: &plan_id,
                    step_id_trim: step.id.trim(),
                    step_index,
                    n,
                    completed_steps_before_this: completed_steps,
                },
                step.executor_kind,
                step_verify_failed_reason.as_deref(),
            )
            .await;

            let reason = if let Err(e) = run_step {
                e.to_string()
            } else {
                step_verify_failed_reason.unwrap_or_else(|| "局部修复耗尽上限".to_string())
            };
            return Err(RunAgentTurnError::StepRetryExhausted {
                phase: AgentTurnSubPhase::Executor,
                message: reason,
            });
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
                &plan_id,
                step.id.trim(),
                step_index,
                n,
                "cancelled",
                step.executor_kind,
                None,
            )
            .await;
            staged_loop_cancelled = true;
            break;
        }

        let tools_ok = staged_step_tool_messages_all_ok(patch_ctx.p.turn.messages, step_user_idx);
        if !tools_ok
            && patch_ctx.p.ctx.cfg.staged_plan_feedback_mode == StagedPlanFeedbackMode::PatchPlanner
        {
            let mut recovered = false;
            for _ in 0..patch_ctx.p.ctx.cfg.staged_plan_patch_max_attempts {
                let feedback = staged_plan_step_failure_feedback_user_body(
                    &plan_id,
                    i,
                    n,
                    &step,
                    "本步内工具调用未全部成功",
                    "请阅读本步对应的 `role: tool` 输出（含失败原因），修订从当前步起的 `steps`（可替换、拆分或追加一步）。",
                );
                if let Some(merged) =
                    run_staged_plan_patch_planner_round(&mut patch_ctx, feedback, &plan_steps, i)
                        .await?
                {
                    plan_steps = merged;
                    n = plan_steps.len();
                    let replan = AgentReplyPlanV1 {
                        plan_type: "agent_reply_plan".to_string(),
                        version: 1,
                        steps: plan_steps.clone(),
                        no_task: false,
                    };
                    let json = plan_artifact::agent_reply_plan_v1_to_json_string(&replan).map_err(
                        |e| RunAgentTurnError::Other {
                            phase: AgentTurnSubPhase::Executor,
                            message: e.to_string(),
                        },
                    )?;
                    push_assistant_merging_trailing_empty_placeholder(
                        patch_ctx.p.turn.messages,
                        Message::assistant_only(json),
                    );
                    send_staged_plan_notice(
                        patch_ctx.p.ctx.out,
                        echo_terminal_staged,
                        true,
                        staged_plan_queue_summary_text(&replan, completed_steps),
                    )
                    .await;
                    recovered = true;
                    break;
                }
            }
            if recovered {
                continue;
            }
            finish_staged_plan_step_failed_and_plan_failed_sse(
                StagedPlanStepFailedExit {
                    out: patch_ctx.p.ctx.out,
                    plan_id: &plan_id,
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
                message: "局部修复耗尽上限 (工具执行失败)".to_string(),
            });
        }

        finish_staged_plan_step_sse(
            patch_ctx.p.ctx.out,
            &plan_id,
            step.id.trim(),
            step_index,
            n,
            "ok",
            step.executor_kind,
            None,
        )
        .await;
        completed_steps = step_index;
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
        i += 1;
    }
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
    let (mut msg, finish_reason) = {
        let (mut first_msg, first_finish) =
            complete_planner_no_tools_chat_retrying(p, &req, planner_render_to_terminal).await?;
        if first_finish != USER_CANCELLED_FINISH_REASON {
            let first_raw_count = first_msg.tool_calls.as_ref().map(|c| c.len()).unwrap_or(0);
            first_msg.tool_calls = None;
            crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(
                &mut first_msg,
                p.ctx.cfg.materialize_deepseek_dsml_tool_calls,
            );
            let first_dsml_count = first_msg.tool_calls.as_ref().map(|c| c.len()).unwrap_or(0);
            let first_total = first_raw_count.saturating_add(first_dsml_count);
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
        }
    };

    debug!(
        target: "crabmate",
        "{} finish_reason={} assistant_preview={}",
        labels.planning_log_label,
        finish_reason,
        crate::redact::assistant_message_preview_for_log(&msg)
    );

    if finish_reason == USER_CANCELLED_FINISH_REASON {
        return Ok(StagedPlanRunOutcome::Finished);
    }

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
        &mut msg,
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

    let merged_for_log =
        crate::agent::plan_artifact::assistant_merged_text_for_plan_artifact_parse(&msg);
    let validate_only_binding_ids =
        plan_rewrite::last_workflow_validate_binding_plan_node_ids(p.turn.messages);
    let plan = match crate::agent::plan_artifact::parse_agent_reply_plan_v1_from_assistant_message_with_validate_only_binding_ids(
        &msg,
        validate_only_binding_ids.as_deref(),
    ) {
        Ok(plan_v1) => plan_v1,
        Err(parse_err) => {
            let detail = crate::agent::plan_artifact::plan_artifact_error_log_summary(&parse_err);
            if matches!(
                parse_err,
                crate::agent::plan_artifact::PlanArtifactError::NotFound
            ) {
                if should_finish_when_plan_not_found(entered_from_step_execution_round) {
                    debug!(
                        target: "crabmate",
                        "分阶段重规划：检测到分步执行后重入且本轮未产出结构化计划，视为收敛完成，直接结束（避免重复总结）"
                    );
                    return Ok(StagedPlanRunOutcome::Finished);
                }
                debug!(
                    target: "crabmate",
                    "分阶段规划未产出结构化任务 (可能是通识问答或直接回复) merged_len={} merged_preview={}；降级为常规循环",
                    merged_for_log.chars().count(),
                    crate::redact::preview_chars(
                        merged_for_log.as_str(),
                        crate::redact::MESSAGE_LOG_PREVIEW_CHARS,
                    )
                );
            } else {
                warn!(
                    target: "crabmate",
                    "staged_plan_invalid parse_err={} merged_len={} merged_preview={}；降级为常规工具循环",
                    detail,
                    merged_for_log.chars().count(),
                    crate::redact::preview_chars(
                        merged_for_log.as_str(),
                        crate::redact::MESSAGE_LOG_PREVIEW_CHARS,
                    )
                );
            }
            // 保留规划轮正文，避免整轮失败退出（REPL/脚本/Web 均与关闭分阶段规划时的行为对齐）。
            push_assistant_merging_trailing_empty_placeholder(p.turn.messages, msg.clone());
            run_agent_outer_loop(p, per_coord).await?;
            return Ok(StagedPlanRunOutcome::Finished);
        }
    };

    let omit_no_task_planner_from_history = p.ctx.out.is_some()
        && !crate::web::web_ui_env::web_raw_assistant_output_env()
        && plan.no_task;
    if !omit_no_task_planner_from_history {
        push_assistant_merging_trailing_empty_placeholder(p.turn.messages, msg.clone());
    }

    if plan.no_task {
        if p.ctx.cfg.staged_plan_two_phase_nl_display {
            run_staged_plan_nl_followup_round(p, per_coord, &make_step_user_message).await?;
        }
        debug!(
            target: "crabmate",
            "分阶段规划：no_task=true，跳过分步注入，转入常规对话循环"
        );
        run_agent_outer_loop(p, per_coord).await?;
        return Ok(StagedPlanRunOutcome::Finished);
    }

    let mut plan = plan;

    let parallel_csv = plan_optimizer::parallel_batchable_tool_names_csv_from_defs(
        p.ctx.tools_defs,
        p.ctx.cfg.as_ref(),
    );
    let validate_only_binding_active =
        plan_rewrite::last_workflow_validate_binding_plan_node_ids(p.turn.messages)
            .is_some_and(|ids| !ids.is_empty());
    let trigger_user = plan_optimizer::staged_plan_trigger_user_content(p.turn.messages);
    let ensemble_route = staged_plan_ensemble_route(
        p.ctx.staged_plan_ensemble_count,
        p.ctx.staged_plan_skip_ensemble_on_casual_prompt,
        validate_only_binding_active,
        trigger_user,
    );
    match ensemble_route {
        StagedPlanEnsembleRoute::SkipValidateOnlyBinding => {
            debug!(
                target: "crabmate",
                "分阶段规划·逻辑多规划员：检测到 workflow_validate_only 节点绑定上下文，跳过 ensemble 以保持逐步绑定稳定"
            );
        }
        StagedPlanEnsembleRoute::SkipCasualHeuristic => {
            debug!(
                target: "crabmate",
                "分阶段规划·逻辑多规划员：用户输入偏短/寒暄启发式，跳过 ensemble（staged_plan_ensemble_count={}）以省 API",
                p.ctx.staged_plan_ensemble_count
            );
        }
        StagedPlanEnsembleRoute::SkipNotConfigured | StagedPlanEnsembleRoute::Run => {}
    }

    if !matches!(
        ensemble_route,
        StagedPlanEnsembleRoute::SkipValidateOnlyBinding
    ) {
        let skip_ensemble_for_casual =
            matches!(ensemble_route, StagedPlanEnsembleRoute::SkipCasualHeuristic);
        maybe_run_staged_plan_ensemble_then_merge(
            p,
            per_coord,
            &labels,
            &make_step_user_message,
            planner_render_to_terminal,
            &mut plan,
            skip_ensemble_for_casual,
        )
        .await?;
    }

    let optimizer_route = staged_plan_optimizer_route(
        plan.steps.len(),
        p.ctx.staged_plan_optimizer_round,
        validate_only_binding_active,
        p.ctx.staged_plan_optimizer_requires_parallel_tools,
        parallel_csv.as_str(),
    );
    match optimizer_route {
        StagedPlanOptimizerRoute::SkipValidateOnlyBinding => {
            debug!(
                target: "crabmate",
                "分阶段规划优化轮：检测到 workflow_validate_only 节点绑定上下文，跳过优化轮以避免破坏绑定约束"
            );
        }
        StagedPlanOptimizerRoute::SkipNoParallelTools => {
            debug!(
                target: "crabmate",
                "分阶段规划优化轮：本会话无可同轮并行批处理的内建工具，跳过优化轮以省 API（步数={}）",
                plan.steps.len()
            );
        }
        StagedPlanOptimizerRoute::SkipStepsLt2
        | StagedPlanOptimizerRoute::SkipOptimizerRoundDisabled
        | StagedPlanOptimizerRoute::Run => {}
    }

    if matches!(optimizer_route, StagedPlanOptimizerRoute::Run) {
        let opt_body =
            plan_optimizer::staged_plan_optimizer_user_body(&plan, parallel_csv.as_str());
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
            return Ok(StagedPlanRunOutcome::Finished);
        }
        strip_staged_planner_message_tool_calls(
            &mut opt_msg,
            "优化轮",
            p.ctx.cfg.materialize_deepseek_dsml_tool_calls,
        );
        let opt_content = crate::types::message_content_as_str(&opt_msg.content).unwrap_or("");
        if let Some(merged_steps) = plan_optimizer::try_parse_optimizer_reply(opt_content) {
            if merged_steps.len() < plan.steps.len() {
                debug!(
                    target: "crabmate",
                    "分阶段规划优化轮：步数 {} -> {}",
                    plan.steps.len(),
                    merged_steps.len()
                );
            }
            push_assistant_merging_trailing_empty_placeholder(p.turn.messages, opt_msg);
            plan.steps = merged_steps;
        } else {
            warn!(
                target: "crabmate",
                "分阶段规划优化轮：未解析出合法 agent_reply_plan v1 或非空 steps，沿用首轮规划"
            );
            pop_last_staged_planner_coach_user_if_present(p.turn.messages);
        }
    }

    if p.ctx.cfg.staged_plan_two_phase_nl_display {
        run_staged_plan_nl_followup_round(p, per_coord, &make_step_user_message).await?;
    }

    let plan_id = next_staged_plan_id();
    let plan_steps = plan.steps;
    let original_steps = plan_steps.clone();
    let patch_ctx = StagedPlanPatchPlannerCtx {
        p,
        per_coord,
        labels: &labels,
        planner_render_to_terminal,
        make_step_user_message: &make_step_user_message,
    };

    run_staged_plan_steps_loop(
        plan_id,
        plan_steps,
        original_steps,
        echo_terminal_staged,
        &labels,
        patch_ctx,
        &make_step_user_message,
    )
    .await
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
            return Err(RunAgentTurnError::Other {
                phase: AgentTurnSubPhase::Planner,
                message: format!(
                    "逻辑双Agent分阶段单步规划轮次超过上限（{}），已停止以避免无限循环",
                    STAGED_SINGLE_STEP_MAX_ROUNDS
                ),
            });
        }
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
            Message::user_only,
        )
        .await;

        let event = match res {
            Ok(o) => StagedTurnSubCallOutcome::Ok(o),
            Err(e) => StagedTurnSubCallOutcome::Err(e),
        };
        let advance =
            advance_staged_turn_after_sub_call(phase, rewrite_attempts, max_rewrites, event);
        rewrite_attempts = next_rewrite_attempts_after_advance(rewrite_attempts, &advance);

        match advance {
            StagedTurnAdvance::Continue {
                phase: next_phase,
                push_feedback_user,
            } => {
                phase = next_phase;
                if let Some(u) = push_feedback_user {
                    if let Some(ref snap) = snapshot {
                        if let Err(e) = snap.restore() {
                            tracing::warn!(target: "crabmate", "逻辑双Agent快照回滚失败: {}", e);
                        } else {
                            tracing::info!(target: "crabmate", "全局重规划触发，工作区已回滚到快照状态");
                        }
                    }
                    p.turn.messages.push(u);
                } else if matches!(phase, StagedTurnPhase::AfterStepExecutionRound) {
                    debug!(
                        target: "crabmate",
                        "逻辑双Agent分阶段单步：本轮执行完成，进入下一轮无工具规划（round={}）",
                        staged_rounds
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

#[cfg(test)]
mod staged_not_found_convergence_tests {
    use super::should_finish_when_plan_not_found;

    #[test]
    fn not_found_does_not_finish_for_plain_qa_round() {
        assert!(
            !should_finish_when_plan_not_found(false),
            "普通问答轮（未进入步后重规划）遇到 NotFound 不应直接收敛结束"
        );
    }

    #[test]
    fn not_found_finishes_only_after_step_execution_reentry() {
        assert!(
            should_finish_when_plan_not_found(true),
            "仅在同 turn 的步后重规划轮，NotFound 才应触发收敛结束"
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
