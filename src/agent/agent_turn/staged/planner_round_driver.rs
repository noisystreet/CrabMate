//! 无工具规划轮（首轮 / ensemble / 优化 / 两阶段 NL）的 LLM 调用与副作用。
//! 从 `mod.rs` 抽出，集中隐式分支（SSE 门控、tool_calls 拒绝重写、ensemble 链），便于后续再向显式 FSM 对齐。

use log::{debug, warn};
use tokio::sync::mpsc;

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::{self, AgentReplyPlanV1};
use crate::agent::plan_ensemble;
use crate::llm::{
    LlmCompleteError, LlmRetryingTransportOpts, kimi_k2_5_vendor_requires_tool_call_reasoning,
    no_tools_chat_request_from_messages,
};
use crate::sse::{SsePayload, encode_message};
use crate::types::{
    Message, USER_CANCELLED_FINISH_REASON, messages_for_api_stripping_reasoning_skip_ui_separators,
};

use super::super::errors::{AgentTurnSubPhase, RunAgentTurnError};
use super::super::params::RunLoopParams;
use super::super::plan::agent_llm_call::AgentLlmCall;
use super::ensemble_fsm::{
    EnsembleMergeOutcome, EnsembleSecondaryPlannerRoundOutcome,
    ensemble_merge_outcome_from_parsed_steps, ensemble_secondary_planner_round_outcome,
};
use super::ensemble_schedule_fsm::{
    EnsembleDriverPhase, ensemble_merge_should_run, ensemble_secondary_planner_display_index,
    resolve_ensemble_driver_phase,
};
use super::prepare_staged_planner_no_tools_request;
use super::sse::staged_plan_nl_followup_user_body;
use crate::agent::reflection::plan_rewrite;

use super::StagedPlanRunLabels;

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

pub(super) async fn emit_staged_planner_tool_call_rejected_timeline(
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

/// 两阶段 NL 开启时：无工具规划轮不向 Web/终端流式下发（由 NL 补全轮承担用户可见输出）。
fn staged_planner_sse_fully_suppressed(cfg: &crate::config::AgentConfig) -> bool {
    cfg.staged_planning.staged_plan_two_phase_nl_display
}

/// 无工具规划轮 `complete_chat_retrying`：
/// - **两阶段 NL**：`out: None`（整段抑制）；
/// - **Web + 未** `CM_WEB_RAW_ASSISTANT_OUTPUT`：经 [`super::super::plan::PlannerSseGate`] — 解析（正文+思维链）为 `no_task` 则整轮不落 SSE，且不将本条 assistant 写入会话；否则仅落 `assistant_answer_phase` 之后的正文增量；
/// - **RAW** 或 **非 Web**：`out: p.ctx.out`（整段原样下发）。
pub(super) async fn complete_planner_no_tools_chat_retrying(
    p: &RunLoopParams<'_>,
    req: &crate::types::ChatRequest,
    planner_render_to_terminal: bool,
) -> Result<(Message, String), LlmCompleteError> {
    let suppress_full = staged_planner_sse_fully_suppressed(p.ctx.cfg.as_ref());
    let use_gate = p.ctx.out.is_some()
        && !crate::web::web_ui_env::web_raw_assistant_output_env()
        && !suppress_full;

    let gate_opt = match (use_gate, p.ctx.out.as_ref()) {
        (true, Some(out)) => Some(super::super::plan::PlannerSseGate::spawn((*out).clone())),
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
pub(super) async fn run_staged_plan_nl_followup_round<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    make_step_user_message: &F,
) -> Result<(), RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let mark = p.turn.messages.len();
    p.turn
        .push_message(make_step_user_message(staged_plan_nl_followup_user_body()));
    let result: Result<(), RunAgentTurnError> = async {
        crate::agent::context_window::prepare_messages_for_model(
            p.ctx.llm_backend,
            p.ctx.client,
            p.ctx.api_key,
            p.ctx.cfg.as_ref(),
            p.turn.messages,
            p.ctx.workspace_changelist.as_ref().map(|a| a.as_ref()),
            crate::agent::context_window::PrepareMessagesForModelHooks {
                per_coord_layer_cache: Some(per_coord),
                run_loop_messages_revision: Some(&mut p.turn.messages_revision),
            },
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
            p.turn.pop_message();
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
            p.ctx
                .cfg
                .dsml_materialize
                .materialize_deepseek_dsml_tool_calls,
        );
        if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
            warn!(
                target: "crabmate",
                "分阶段规划·自然语言补全轮：DSML 物化出 tool_calls，已忽略"
            );
            msg.tool_calls = None;
        }
        p.turn.push_assistant_merging_trailing_empty(msg);
        Ok(())
    }
    .await;
    if result.is_err() && p.turn.messages.len() > mark {
        p.turn.truncate_messages(mark);
    }
    result
}

/// 无工具规划补全：假定 `p.turn.messages` 已含本轮所需的 user（若有）；与 `prepare_staged_planner_no_tools_request` + `complete_planner_no_tools_chat_retrying` 一致。
pub(super) async fn complete_one_staged_planner_assistant_round(
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
pub(super) fn strip_staged_planner_message_tool_calls(
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
pub(super) async fn maybe_run_staged_plan_ensemble_then_merge<F>(
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

    let dsml = p
        .ctx
        .cfg
        .dsml_materialize
        .materialize_deepseek_dsml_tool_calls;
    let mut accepted: Vec<AgentReplyPlanV1> = vec![plan.clone()];

    for i in 0..extra {
        let planner_idx = ensemble_secondary_planner_display_index(i);
        let body = plan_ensemble::ensemble_secondary_planner_user_body(planner_idx, &accepted);
        p.turn.push_message(make_step_user_message(body));
        let (mut sec_msg, fin) = complete_one_staged_planner_assistant_round(
            p,
            per_coord,
            labels.build_planner_messages,
            planner_render_to_terminal,
            "分阶段规划·逻辑多规划员轮",
        )
        .await?;
        if fin == USER_CANCELLED_FINISH_REASON {
            p.turn.pop_last_staged_planner_coach_user_if_present();
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
                p.turn.pop_last_staged_planner_coach_user_if_present();
                accepted.push(p2);
            }
            EnsembleSecondaryPlannerRoundOutcome::StopChain => {
                warn!(
                    target: "crabmate",
                    "分阶段规划·逻辑多规划员：第 {} 份规划解析失败或无效，停止追加规划员（保留已收集的 {} 份）",
                    planner_idx,
                    accepted.len()
                );
                p.turn.pop_last_staged_planner_coach_user_if_present();
                break;
            }
        }
    }

    if !ensemble_merge_should_run(accepted.len()) {
        return Ok(());
    }

    let merge_body = plan_ensemble::ensemble_merge_planner_user_body(&accepted);
    p.turn.push_message(make_step_user_message(merge_body));
    let (mut merge_msg, merge_fin) = complete_one_staged_planner_assistant_round(
        p,
        per_coord,
        labels.build_planner_messages,
        planner_render_to_terminal,
        "分阶段规划·多规划合并轮",
    )
    .await?;
    if merge_fin == USER_CANCELLED_FINISH_REASON {
        p.turn.pop_last_staged_planner_coach_user_if_present();
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
            p.turn.push_assistant_merging_trailing_empty(merge_msg);
            plan.steps = steps;
        }
        EnsembleMergeOutcome::KeepPriorPlan => {
            warn!(
                target: "crabmate",
                "分阶段规划·多规划合并：未解析出合法 agent_reply_plan，沿用合并前规划（{} 步）",
                plan.steps.len()
            );
            p.turn.pop_last_staged_planner_coach_user_if_present();
        }
    }
    Ok(())
}

/// 首轮无工具规划轮：可选 tool_calls 重写后再返回 assistant。
pub(super) async fn complete_first_planner_round_maybe_retry_tool_reject<F>(
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
            p.ctx
                .cfg
                .dsml_materialize
                .materialize_deepseek_dsml_tool_calls,
        );
        if first_total > 0 {
            warn!(
                target: "crabmate",
                "分阶段规划轮：检测到 {} 条 tool_calls，严格无工具模式触发一次轻量重写",
                first_total
            );
            emit_staged_planner_tool_call_rejected_timeline(p.ctx.out, first_total).await;
            p.turn.push_message(make_step_user_message(
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
