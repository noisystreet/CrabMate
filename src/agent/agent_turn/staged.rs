//! 分阶段规划与逻辑双 agent：规划轮 + 逐步注入执行。

use std::sync::atomic::Ordering;

use log::{debug, warn};
use tokio::sync::mpsc;

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::{self, AgentReplyPlanV1, PlanStepV1};
use crate::agent::plan_ensemble;
use crate::agent::plan_optimizer::{self, STAGED_PLAN_OPTIMIZER_COACH_MARK};
use crate::config::StagedPlanFeedbackMode;
use crate::llm::{
    CompleteChatRetryingParams, complete_chat_retrying,
    kimi_k2_5_vendor_requires_tool_call_reasoning, no_tools_chat_request_from_messages,
};
use crate::sse::{SseErrorBody, SsePayload, encode_message};
use crate::tool_result::tool_message_content_ok_for_model;
use crate::types::{
    Message, USER_CANCELLED_FINISH_REASON, is_message_excluded_from_llm_context_except_memory,
    message_clone_stripping_reasoning_for_api,
    messages_for_api_stripping_reasoning_skip_ui_separators,
};

use super::execute_tools::{
    ExecuteToolsBatchOutcome, WebExecuteCtx, per_execute_tools_web, sse_sender_closed,
};
use super::messages::push_assistant_merging_trailing_empty_placeholder;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;
use super::reflect::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
use super::staged_sse::{
    emit_chat_ui_separator_sse, next_staged_plan_id, send_staged_plan_finished,
    send_staged_plan_notice, send_staged_plan_started, send_staged_plan_step_finished,
    send_staged_plan_step_started, staged_plan_nl_followup_user_body,
    staged_plan_phase_instruction_default, staged_plan_queue_summary_text,
};

/// 若最后一条为带「规划教练」标记的临时 user，则弹出（取消或解析失败时避免孤立上下文）。
fn pop_last_staged_planner_coach_user_if_present(messages: &mut Vec<Message>) {
    if let Some(last) = messages.last()
        && last.role == "user"
        && last.content.as_deref().is_some_and(|c| {
            c.contains(STAGED_PLAN_OPTIMIZER_COACH_MARK)
                || plan_ensemble::is_ensemble_injected_user_content(c)
        })
    {
        messages.pop();
    }
}

fn staged_two_phase_suppress_planner_stream(cfg: &crate::config::AgentConfig) -> bool {
    cfg.staged_plan_two_phase_nl_display
}

/// `staged_plan_two_phase_nl_display` 开启时：在上下文中已有规划 JSON 助手条后，追加一轮仅自然语言的**用户可见**输出（SSE/终端）。
async fn run_staged_plan_nl_followup_round<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    make_step_user_message: &F,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    F: Fn(String) -> Message,
{
    let mark = p.messages.len();
    p.messages
        .push(make_step_user_message(staged_plan_nl_followup_user_body()));
    let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = async {
        crate::agent::context_window::prepare_messages_for_model(
            p.llm_backend,
            p.client,
            p.api_key,
            p.cfg.as_ref(),
            p.messages,
            Some(per_coord),
            p.workspace_changelist.as_ref().map(|a| a.as_ref()),
        )
        .await?;
        let stripped = messages_for_api_stripping_reasoning_skip_ui_separators(
            p.messages.as_slice(),
            kimi_k2_5_vendor_requires_tool_call_reasoning(p.cfg.as_ref()),
        );
        let req = no_tools_chat_request_from_messages(
            p.cfg.as_ref(),
            stripped,
            p.temperature_override,
            p.seed_override,
        );
        let cc = CompleteChatRetryingParams {
            llm_backend: p.llm_backend,
            http: p.client,
            api_key: p.api_key,
            cfg: p.cfg.as_ref(),
            out: p.out,
            render_to_terminal: p.render_to_terminal,
            no_stream: p.no_stream,
            cancel: p.cancel,
            plain_terminal_stream: p.plain_terminal_stream,
            request_chrome_trace: p.request_chrome_trace.clone(),
        };
        let (mut msg, finish_reason) = complete_chat_retrying(&cc, &req).await?;
        if finish_reason == USER_CANCELLED_FINISH_REASON {
            p.messages.pop();
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
            p.cfg.materialize_deepseek_dsml_tool_calls,
        );
        if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
            warn!(
                target: "crabmate",
                "分阶段规划·自然语言补全轮：DSML 物化出 tool_calls，已忽略"
            );
            msg.tool_calls = None;
        }
        push_assistant_merging_trailing_empty_placeholder(p.messages, msg);
        Ok(())
    }
    .await;
    if result.is_err() && p.messages.len() > mark {
        p.messages.truncate(mark);
    }
    result
}

/// 无工具规划补全：假定 `p.messages` 已含本轮所需的 user（若有）；与 `prepare_staged_planner_no_tools_request` + `complete_chat_retrying` 一致。
async fn complete_one_staged_planner_assistant_round(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    build_planner_messages: fn(&[Message], String, bool) -> Vec<Message>,
    planner_render_to_terminal: bool,
    log_label: &'static str,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let req = prepare_staged_planner_no_tools_request(p, per_coord, build_planner_messages).await?;
    let suppress = staged_two_phase_suppress_planner_stream(p.cfg.as_ref());
    let cc = CompleteChatRetryingParams {
        llm_backend: p.llm_backend,
        http: p.client,
        api_key: p.api_key,
        cfg: p.cfg.as_ref(),
        out: if suppress { None } else { p.out },
        render_to_terminal: planner_render_to_terminal && !suppress,
        no_stream: p.no_stream,
        cancel: p.cancel,
        plain_terminal_stream: p.plain_terminal_stream,
        request_chrome_trace: p.request_chrome_trace.clone(),
    };
    let (msg, finish_reason) = complete_chat_retrying(&cc, &req).await?;
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    F: Fn(String) -> Message,
{
    let extra = p.staged_plan_ensemble_count.saturating_sub(1);
    if extra == 0 {
        return Ok(());
    }
    if skip_for_casual_user_prompt {
        debug!(
            target: "crabmate",
            "分阶段规划·逻辑多规划员：用户输入偏短/寒暄启发式，跳过 ensemble（staged_plan_ensemble_count={}）以省 API",
            p.staged_plan_ensemble_count
        );
        return Ok(());
    }

    let dsml = p.cfg.materialize_deepseek_dsml_tool_calls;
    let mut accepted: Vec<AgentReplyPlanV1> = vec![plan.clone()];

    for i in 0..extra {
        let planner_idx = i.saturating_add(2);
        let body = plan_ensemble::ensemble_secondary_planner_user_body(planner_idx, &accepted);
        p.messages.push(make_step_user_message(body));
        let (mut sec_msg, fin) = complete_one_staged_planner_assistant_round(
            p,
            per_coord,
            labels.build_planner_messages,
            planner_render_to_terminal,
            "分阶段规划·逻辑多规划员轮",
        )
        .await?;
        if fin == USER_CANCELLED_FINISH_REASON {
            pop_last_staged_planner_coach_user_if_present(p.messages);
            return Ok(());
        }
        strip_staged_planner_message_tool_calls(&mut sec_msg, "·逻辑多规划员", dsml);
        let content = sec_msg.content.as_deref().unwrap_or("");
        match plan_artifact::parse_agent_reply_plan_v1(content) {
            Ok(p2) if !p2.no_task && !p2.steps.is_empty() => {
                pop_last_staged_planner_coach_user_if_present(p.messages);
                accepted.push(p2);
            }
            Ok(_) | Err(_) => {
                warn!(
                    target: "crabmate",
                    "分阶段规划·逻辑多规划员：第 {} 份规划解析失败或无效，停止追加规划员（保留已收集的 {} 份）",
                    planner_idx,
                    accepted.len()
                );
                pop_last_staged_planner_coach_user_if_present(p.messages);
                break;
            }
        }
    }

    if accepted.len() < 2 {
        return Ok(());
    }

    let merge_body = plan_ensemble::ensemble_merge_planner_user_body(&accepted);
    p.messages.push(make_step_user_message(merge_body));
    let (mut merge_msg, merge_fin) = complete_one_staged_planner_assistant_round(
        p,
        per_coord,
        labels.build_planner_messages,
        planner_render_to_terminal,
        "分阶段规划·多规划合并轮",
    )
    .await?;
    if merge_fin == USER_CANCELLED_FINISH_REASON {
        pop_last_staged_planner_coach_user_if_present(p.messages);
        return Ok(());
    }
    strip_staged_planner_message_tool_calls(&mut merge_msg, "·多规划合并", dsml);
    let merge_content = merge_msg.content.as_deref().unwrap_or("");
    if let Some(steps) = plan_ensemble::try_parse_ensemble_planner_reply(merge_content) {
        debug!(
            target: "crabmate",
            "分阶段规划·多规划合并：步数 {} -> {}（来自 {} 份草案）",
            plan.steps.len(),
            steps.len(),
            accepted.len()
        );
        push_assistant_merging_trailing_empty_placeholder(p.messages, merge_msg);
        plan.steps = steps;
    } else {
        warn!(
            target: "crabmate",
            "分阶段规划·多规划合并：未解析出合法 agent_reply_plan，沿用合并前规划（{} 步）",
            plan.steps.len()
        );
        pop_last_staged_planner_coach_user_if_present(p.messages);
    }
    Ok(())
}

/// 分阶段规划共享执行路径上的日志文案（避免 `run_staged_plan_with_prepared_request` 参数过长）。
pub(crate) struct StagedPlanRunLabels {
    pub planning_log_label: &'static str,
    pub step_injection_log_label: &'static str,
    pub build_planner_messages: fn(&[Message], String, bool) -> Vec<Message>,
}

async fn prepare_staged_planner_no_tools_request(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    build_planner_messages: fn(&[Message], String, bool) -> Vec<Message>,
) -> Result<crate::types::ChatRequest, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(ref ltm) = p.long_term_memory {
        ltm.prepare_messages(
            p.cfg.as_ref(),
            p.long_term_memory_scope_id.as_deref(),
            p.messages,
        );
    }
    crate::agent::context_window::prepare_messages_for_model(
        p.llm_backend,
        p.client,
        p.api_key,
        p.cfg.as_ref(),
        p.messages,
        Some(per_coord),
        p.workspace_changelist.as_ref().map(|a| a.as_ref()),
    )
    .await?;

    let instr = p.cfg.staged_plan_phase_instruction.trim();
    let plan_system = if instr.is_empty() {
        staged_plan_phase_instruction_default()
    } else {
        instr.to_string()
    };
    let preserve = crate::llm::llm_vendor_adapter(p.cfg.as_ref())
        .preserve_assistant_tool_call_reasoning(p.cfg.as_ref());
    Ok(no_tools_chat_request_from_messages(
        p.cfg.as_ref(),
        build_planner_messages(p.messages, plan_system, preserve),
        p.temperature_override,
        p.seed_override,
    ))
}

pub(super) async fn run_staged_plan_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let render_to_terminal = p.render_to_terminal;
    let echo_terminal_staged = render_to_terminal && p.out.is_none();

    let labels = StagedPlanRunLabels {
        planning_log_label: "分阶段规划轮模型输出",
        step_injection_log_label: "分步注入 user（完整正文，供排障与日志）",
        build_planner_messages: build_single_agent_planner_messages,
    };
    let req = prepare_staged_planner_no_tools_request(p, per_coord, labels.build_planner_messages)
        .await?;
    run_staged_plan_with_prepared_request(
        p,
        per_coord,
        req,
        render_to_terminal,
        echo_terminal_staged,
        labels,
        |body| Message {
            role: "user".to_string(),
            content: Some(body),
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
) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_message_excluded_from_llm_context_except_memory(m))
        .map(|m| {
            message_clone_stripping_reasoning_for_api(m, preserve_reasoning_on_assistant_tool_calls)
        })
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

pub(crate) fn build_logical_dual_planner_messages(
    messages: &[Message],
    plan_system: String,
    preserve_reasoning_on_assistant_tool_calls: bool,
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
            m.content
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        })
        .map(|m| {
            message_clone_stripping_reasoning_for_api(m, preserve_reasoning_on_assistant_tool_calls)
        })
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

/// 发送单步结束 SSE（`failed` / `cancelled` / `ok`）。
async fn finish_staged_plan_step_sse(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id_trim: &str,
    step_index: usize,
    n: usize,
    status: &'static str,
) {
    send_staged_plan_step_finished(out, plan_id, step_id_trim, step_index, n, status).await;
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

async fn finish_staged_plan_step_failed_and_plan_failed_sse(f: StagedPlanStepFailedExit<'_>) {
    finish_staged_plan_step_sse(
        f.out,
        f.plan_id,
        f.step_id_trim,
        f.step_index,
        f.n,
        "failed",
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
            let content = m.content.as_deref().unwrap_or("");
            if !tool_message_content_ok_for_model(content, "") {
                return false;
            }
        }
        i += 1;
    }
    true
}

fn staged_plan_step_failure_feedback_user_body(
    plan_id: &str,
    step_zero_based: usize,
    n: usize,
    step: &PlanStepV1,
    reason_zh: &str,
    detail: &str,
) -> String {
    format!(
        "### 分阶段规划 · 步级反馈（plan_id={}）\n\
         当前执行步 **{}/{}**（零基下标 {}）未顺利完成。\n\
         - 失败原因：{}\n\
         - 详情摘要：{}\n\
         - 当前步 id：`{}`\n\
         - 当前步描述：{}\n\n\
         请作为**规划器**仅输出一段可解析的 `agent_reply_plan` v1 JSON（可用 ```json 围栏）。\n\
         **补丁规则**：`steps` 数组表示从**本步起**的后续计划（可替换原剩余步骤、在末尾增加一步、或合并/拆分步骤）；须 **非空** 且 **不得** 使用 `no_task`。\n\
         已完成的前缀步（下标 0..{}）已由服务端保留，你**不要**在 `steps` 中重复列出。\n\n\
         Schema 须满足：{}\n\
         示例：\n```json\n{}\n```",
        plan_id,
        step_zero_based + 1,
        n,
        step_zero_based,
        reason_zh,
        detail,
        step.id.trim(),
        step.description.trim(),
        step_zero_based,
        plan_artifact::PLAN_V1_SCHEMA_RULES,
        plan_artifact::PLAN_V1_EXAMPLE_JSON
    )
}

/// 分阶段规划补丁轮入参（控制 clippy `too_many_arguments`）。
struct StagedPlanPatchPlannerCtx<'p, 'a, F> {
    p: &'p mut RunLoopParams<'a>,
    per_coord: &'p mut PerCoordinator,
    labels: &'p StagedPlanRunLabels,
    /// 子循环与规划轮内工具执行的 CLI 转录开关（与外层 `render_to_terminal` 一致）。
    render_to_terminal: bool,
    /// 仅用于补丁轮 `complete_chat_retrying`：CLI 可单独关闭规划模型 stdout。
    planner_render_to_terminal: bool,
    make_step_user_message: &'p F,
}

/// 追加反馈 user 后跑一轮无工具规划；成功则返回合并后的 `steps`，失败返回 `Ok(None)`（调用方按补丁次数用尽处理）。
async fn run_staged_plan_patch_planner_round<F>(
    ctx: &mut StagedPlanPatchPlannerCtx<'_, '_, F>,
    feedback_user_body: String,
    base_steps: &[PlanStepV1],
    failed_step_zero_based: usize,
) -> Result<Option<Vec<PlanStepV1>>, Box<dyn std::error::Error + Send + Sync>>
where
    F: Fn(String) -> Message,
{
    let StagedPlanPatchPlannerCtx {
        p,
        per_coord,
        labels,
        render_to_terminal,
        planner_render_to_terminal,
        make_step_user_message,
    } = ctx;
    p.messages.push(make_step_user_message(feedback_user_body));
    let req = prepare_staged_planner_no_tools_request(p, per_coord, labels.build_planner_messages)
        .await?;
    let suppress = staged_two_phase_suppress_planner_stream(p.cfg.as_ref());
    let cc = CompleteChatRetryingParams {
        llm_backend: p.llm_backend,
        http: p.client,
        api_key: p.api_key,
        cfg: p.cfg.as_ref(),
        out: if suppress { None } else { p.out },
        render_to_terminal: *planner_render_to_terminal && !suppress,
        no_stream: p.no_stream,
        cancel: p.cancel,
        plain_terminal_stream: p.plain_terminal_stream,
        request_chrome_trace: p.request_chrome_trace.clone(),
    };
    let (mut msg, finish_reason) = complete_chat_retrying(&cc, &req).await?;

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
    msg.tool_calls = None;
    crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(
        &mut msg,
        p.cfg.materialize_deepseek_dsml_tool_calls,
    );

    push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());

    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        match per_reflect_after_assistant(p, per_coord, &finish_reason, &msg).await {
            ReflectOnAssistantOutcome::ProceedToExecuteTools => {
                let tool_calls = msg.tool_calls.as_ref().ok_or("无 tool_calls")?;
                let echo_terminal_transcript = *render_to_terminal && p.out.is_none();
                let exec_outcome = per_execute_tools_web(
                    tool_calls,
                    per_coord,
                    p.messages,
                    WebExecuteCtx {
                        cfg: p.cfg,
                        effective_working_dir: p.effective_working_dir,
                        workspace_is_set: p.workspace_is_set,
                        read_file_turn_cache: p.read_file_turn_cache.clone(),
                        out: p.out,
                        web_tool_ctx: p.web_tool_ctx,
                        cli_tool_ctx: p.cli_tool_ctx,
                        echo_terminal_transcript,
                        mcp_session: p.mcp_session.as_ref(),
                        workspace_changelist: p.workspace_changelist.as_ref(),
                        request_chrome_trace: p.request_chrome_trace.clone(),
                        step_executor_constraint: None,
                    },
                )
                .await;
                if matches!(exec_outcome, ExecuteToolsBatchOutcome::AbortedSse) {
                    return Ok(None);
                }
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
            }
            ReflectOnAssistantOutcome::StopTurn => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                return Ok(None);
            }
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                    f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
                }
                run_agent_outer_loop(p, per_coord).await?;
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
            }
            ReflectOnAssistantOutcome::PlanRewriteExhausted { reason } => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                if let Some(tx) = p.out {
                    let _ = crate::sse::send_string_logged(
                        tx,
                        encode_message(SsePayload::Error(SseErrorBody {
                            error: PerCoordinator::plan_rewrite_exhausted_sse_message().to_string(),
                            code: Some("plan_rewrite_exhausted".to_string()),
                            reason_code: Some(reason.as_str().to_string()),
                        })),
                        "staged::patch_plan_rewrite_exhausted",
                    )
                    .await;
                }
                return Ok(None);
            }
        }
        return Ok(None);
    }

    let content = msg.content.as_deref().unwrap_or("");
    let patch_plan = match plan_artifact::parse_agent_reply_plan_v1(content) {
        Ok(p) => p,
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
        Ok(merged) => Ok(Some(merged)),
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

pub(crate) async fn run_staged_plan_with_prepared_request<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    req: crate::types::ChatRequest,
    render_to_terminal: bool,
    echo_terminal_staged: bool,
    labels: StagedPlanRunLabels,
    make_step_user_message: F,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    F: Fn(String) -> Message,
{
    let planner_render_to_terminal =
        render_to_terminal && (p.out.is_some() || p.cfg.staged_plan_cli_show_planner_stream);
    let suppress = staged_two_phase_suppress_planner_stream(p.cfg.as_ref());
    let cc = CompleteChatRetryingParams {
        llm_backend: p.llm_backend,
        http: p.client,
        api_key: p.api_key,
        cfg: p.cfg.as_ref(),
        out: if suppress { None } else { p.out },
        render_to_terminal: planner_render_to_terminal && !suppress,
        no_stream: p.no_stream,
        cancel: p.cancel,
        plain_terminal_stream: p.plain_terminal_stream,
        request_chrome_trace: p.request_chrome_trace.clone(),
    };
    let (mut msg, finish_reason) = complete_chat_retrying(&cc, &req).await?;

    debug!(
        target: "crabmate",
        "{} finish_reason={} assistant_preview={}",
        labels.planning_log_label,
        finish_reason,
        crate::redact::assistant_message_preview_for_log(&msg)
    );

    if finish_reason == USER_CANCELLED_FINISH_REASON {
        return Ok(());
    }

    // 规划轮请求为 `tools: []` + `tool_choice: none`，但部分网关仍返回**原生** `tool_calls`（含函数名）。
    // `materialize_deepseek_dsml_tool_calls_in_message` 在「已有可用原生 tool_calls」时会直接 return，
    // 导致正文里的 DeepSeek DSML **永不物化**；若此前再按原生判错，CLI（`out: None`）会静默 `return Ok`。
    // 与无工具约束一致：规划轮**忽略**原生 tool_calls，只从正文（及 reasoning）物化 DSML。
    if let Some(tc) = msg.tool_calls.as_ref().filter(|c| !c.is_empty()) {
        debug!(
            target: "crabmate",
            "分阶段规划轮：丢弃 API 返回的 {} 条原生 tool_calls，改从正文 DSML 物化",
            tc.len()
        );
    }
    msg.tool_calls = None;
    crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(
        &mut msg,
        p.cfg.materialize_deepseek_dsml_tool_calls,
    );

    // 规划轮若未产出可解析 JSON，但正文里写了 DSML 工具调用：物化后应先执行工具，再进入常规循环（否则历史中只有未执行的 XML）。
    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());
        match per_reflect_after_assistant(p, per_coord, &finish_reason, &msg).await {
            ReflectOnAssistantOutcome::ProceedToExecuteTools => {
                let tool_calls = msg.tool_calls.as_ref().ok_or("无 tool_calls")?;
                let echo_terminal_transcript = render_to_terminal && p.out.is_none();
                let exec_outcome = per_execute_tools_web(
                    tool_calls,
                    per_coord,
                    p.messages,
                    WebExecuteCtx {
                        cfg: p.cfg,
                        effective_working_dir: p.effective_working_dir,
                        workspace_is_set: p.workspace_is_set,
                        read_file_turn_cache: p.read_file_turn_cache.clone(),
                        out: p.out,
                        web_tool_ctx: p.web_tool_ctx,
                        cli_tool_ctx: p.cli_tool_ctx,
                        echo_terminal_transcript,
                        mcp_session: p.mcp_session.as_ref(),
                        workspace_changelist: p.workspace_changelist.as_ref(),
                        request_chrome_trace: p.request_chrome_trace.clone(),
                        step_executor_constraint: None,
                    },
                )
                .await;
                if matches!(exec_outcome, ExecuteToolsBatchOutcome::AbortedSse) {
                    return Ok(());
                }
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
            }
            ReflectOnAssistantOutcome::StopTurn => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                return Ok(());
            }
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                    f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
                }
                return run_agent_outer_loop(p, per_coord).await;
            }
            ReflectOnAssistantOutcome::PlanRewriteExhausted { reason } => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                if let Some(tx) = p.out {
                    let _ = crate::sse::send_string_logged(
                        tx,
                        encode_message(SsePayload::Error(SseErrorBody {
                            error: PerCoordinator::plan_rewrite_exhausted_sse_message().to_string(),
                            code: Some("plan_rewrite_exhausted".to_string()),
                            reason_code: Some(reason.as_str().to_string()),
                        })),
                        "staged::plan_rewrite_exhausted",
                    )
                    .await;
                }
                return Ok(());
            }
        }
        return run_agent_outer_loop(p, per_coord).await;
    }

    let content = msg.content.as_deref().unwrap_or("");
    let plan = match crate::agent::plan_artifact::parse_agent_reply_plan_v1(content) {
        Ok(plan_v1) => plan_v1,
        Err(parse_err) => {
            let detail = crate::agent::plan_artifact::plan_artifact_error_log_summary(&parse_err);
            warn!(
                target: "crabmate",
                "staged_plan_invalid parse_err={} content_len={} content_preview={}；降级为常规工具循环",
                detail,
                content.chars().count(),
                crate::redact::preview_chars(content, crate::redact::MESSAGE_LOG_PREVIEW_CHARS)
            );
            // 保留规划轮正文，避免整轮失败退出（REPL/脚本/Web 均与关闭分阶段规划时的行为对齐）。
            push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());
            return run_agent_outer_loop(p, per_coord).await;
        }
    };

    push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());

    if plan.no_task {
        if p.cfg.staged_plan_two_phase_nl_display {
            run_staged_plan_nl_followup_round(p, per_coord, &make_step_user_message).await?;
        }
        debug!(
            target: "crabmate",
            "分阶段规划：no_task=true，跳过分步注入，转入常规对话循环"
        );
        return run_agent_outer_loop(p, per_coord).await;
    }

    let mut plan = plan;

    let parallel_csv =
        plan_optimizer::parallel_batchable_tool_names_csv_from_defs(p.tools_defs, p.cfg.as_ref());
    let skip_ensemble_for_casual = p.staged_plan_ensemble_count > 1
        && p.staged_plan_skip_ensemble_on_casual_prompt
        && plan_optimizer::staged_plan_trigger_user_content(p.messages)
            .is_some_and(plan_optimizer::staged_plan_user_prompt_looks_like_casual_or_trivial);

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

    let want_optimizer = plan.steps.len() >= 2 && p.staged_plan_optimizer_round;
    let skip_optimizer_no_parallel_tools = want_optimizer
        && p.staged_plan_optimizer_requires_parallel_tools
        && parallel_csv.trim().is_empty();
    if skip_optimizer_no_parallel_tools {
        debug!(
            target: "crabmate",
            "分阶段规划优化轮：本会话无可同轮并行批处理的内建工具，跳过优化轮以省 API（步数={}）",
            plan.steps.len()
        );
    }
    if want_optimizer && !skip_optimizer_no_parallel_tools {
        let opt_body =
            plan_optimizer::staged_plan_optimizer_user_body(&plan, parallel_csv.as_str());
        p.messages.push(make_step_user_message(opt_body));
        let (mut opt_msg, opt_finish) = complete_one_staged_planner_assistant_round(
            p,
            per_coord,
            labels.build_planner_messages,
            planner_render_to_terminal,
            "分阶段规划优化轮模型输出",
        )
        .await?;
        if opt_finish == USER_CANCELLED_FINISH_REASON {
            pop_last_staged_planner_coach_user_if_present(p.messages);
            return Ok(());
        }
        strip_staged_planner_message_tool_calls(
            &mut opt_msg,
            "优化轮",
            p.cfg.materialize_deepseek_dsml_tool_calls,
        );
        let opt_content = opt_msg.content.as_deref().unwrap_or("");
        if let Some(merged_steps) = plan_optimizer::try_parse_optimizer_reply(opt_content) {
            if merged_steps.len() < plan.steps.len() {
                debug!(
                    target: "crabmate",
                    "分阶段规划优化轮：步数 {} -> {}",
                    plan.steps.len(),
                    merged_steps.len()
                );
            }
            push_assistant_merging_trailing_empty_placeholder(p.messages, opt_msg);
            plan.steps = merged_steps;
        } else {
            warn!(
                target: "crabmate",
                "分阶段规划优化轮：未解析出合法 agent_reply_plan v1 或非空 steps，沿用首轮规划"
            );
            pop_last_staged_planner_coach_user_if_present(p.messages);
        }
    }

    if p.cfg.staged_plan_two_phase_nl_display {
        run_staged_plan_nl_followup_round(p, per_coord, &make_step_user_message).await?;
    }

    let plan_id = next_staged_plan_id();
    let mut plan_steps = plan.steps;
    let mut n = plan_steps.len();
    let mut patch_ctx = StagedPlanPatchPlannerCtx {
        p,
        per_coord,
        labels: &labels,
        render_to_terminal,
        planner_render_to_terminal,
        make_step_user_message: &make_step_user_message,
    };

    send_staged_plan_started(patch_ctx.p.out, &plan_id, n).await;

    let plan_for_notice = AgentReplyPlanV1 {
        plan_type: "agent_reply_plan".to_string(),
        version: 1,
        steps: plan_steps.clone(),
        no_task: false,
    };
    send_staged_plan_notice(
        patch_ctx.p.out,
        echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&plan_for_notice, 0),
    )
    .await;

    let mut staged_loop_cancelled = false;
    let mut completed_steps = 0usize;
    let mut i = 0usize;
    while i < plan_steps.len() {
        if sse_sender_closed(patch_ctx.p.out)
            || patch_ctx.p.cancel.is_some_and(|c| c.load(Ordering::SeqCst))
        {
            staged_loop_cancelled = true;
            break;
        }
        let step = plan_steps[i].clone();
        let step_index = i + 1;
        send_staged_plan_step_started(
            patch_ctx.p.out,
            &plan_id,
            step.id.trim(),
            step_index,
            n,
            step.description.trim(),
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
                "\n- **子代理角色**：`test_runner`（本步仅允许只读工具与内置测试运行器，如 `cargo_test` / `pytest_run` / `go_test` 等；**不要**用 `run_command` 跑测试）\n"
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
        let step_user_idx = patch_ctx.p.messages.len();
        patch_ctx.p.messages.push(make_step_user_message(body));
        let prev_executor_constraint = patch_ctx.p.step_executor_constraint;
        patch_ctx.p.step_executor_constraint = step.executor_kind;
        let run_step = run_agent_outer_loop(patch_ctx.p, patch_ctx.per_coord).await;
        patch_ctx.p.step_executor_constraint = prev_executor_constraint;
        if let Err(e) = run_step {
            if patch_ctx.p.cfg.staged_plan_feedback_mode == StagedPlanFeedbackMode::PatchPlanner {
                let mut recovered = false;
                for _ in 0..patch_ctx.p.cfg.staged_plan_patch_max_attempts {
                    let feedback = staged_plan_step_failure_feedback_user_body(
                        &plan_id,
                        i,
                        n,
                        &step,
                        "执行子循环返回错误",
                        "请根据对话历史缩短或调整后续步骤；若属环境/权限问题请在补丁中显式增加修复步。",
                    );
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
                            .map_err(|e| e.to_string())?;
                        push_assistant_merging_trailing_empty_placeholder(
                            patch_ctx.p.messages,
                            Message::assistant_only(json),
                        );
                        send_staged_plan_notice(
                            patch_ctx.p.out,
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
            finish_staged_plan_step_failed_and_plan_failed_sse(StagedPlanStepFailedExit {
                out: patch_ctx.p.out,
                plan_id: &plan_id,
                step_id_trim: step.id.trim(),
                step_index,
                n,
                completed_steps_before_this: completed_steps,
            })
            .await;
            return Err(e);
        }
        if sse_sender_closed(patch_ctx.p.out)
            || patch_ctx.p.cancel.is_some_and(|c| c.load(Ordering::SeqCst))
        {
            finish_staged_plan_step_sse(
                patch_ctx.p.out,
                &plan_id,
                step.id.trim(),
                step_index,
                n,
                "cancelled",
            )
            .await;
            staged_loop_cancelled = true;
            break;
        }

        let tools_ok = staged_step_tool_messages_all_ok(patch_ctx.p.messages, step_user_idx);
        if !tools_ok
            && patch_ctx.p.cfg.staged_plan_feedback_mode == StagedPlanFeedbackMode::PatchPlanner
        {
            let mut recovered = false;
            for _ in 0..patch_ctx.p.cfg.staged_plan_patch_max_attempts {
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
                    let json = plan_artifact::agent_reply_plan_v1_to_json_string(&replan)
                        .map_err(|e| e.to_string())?;
                    push_assistant_merging_trailing_empty_placeholder(
                        patch_ctx.p.messages,
                        Message::assistant_only(json),
                    );
                    send_staged_plan_notice(
                        patch_ctx.p.out,
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
            finish_staged_plan_step_failed_and_plan_failed_sse(StagedPlanStepFailedExit {
                out: patch_ctx.p.out,
                plan_id: &plan_id,
                step_id_trim: step.id.trim(),
                step_index,
                n,
                completed_steps_before_this: completed_steps,
            })
            .await;
            return Ok(());
        }

        finish_staged_plan_step_sse(
            patch_ctx.p.out,
            &plan_id,
            step.id.trim(),
            step_index,
            n,
            "ok",
        )
        .await;
        completed_steps = step_index;
        patch_ctx.p.messages.push(Message::chat_ui_separator(true));
        let plan_row = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".to_string(),
            version: 1,
            steps: plan_steps.clone(),
            no_task: false,
        };
        send_staged_plan_notice(
            patch_ctx.p.out,
            echo_terminal_staged,
            true,
            staged_plan_queue_summary_text(&plan_row, step_index),
        )
        .await;
        emit_chat_ui_separator_sse(patch_ctx.p.out, true).await;
        i += 1;
    }
    // 末步成功后循环内已发送含「[✓] 全部完成」的摘要，勿再发一次（否则重复一条）。
    send_staged_plan_finished(
        patch_ctx.p.out,
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
        patch_ctx.p.messages.push(Message::chat_ui_separator(true));
        emit_chat_ui_separator_sse(patch_ctx.p.out, true).await;
    }
    Ok(())
}

pub(super) async fn run_logical_dual_agent_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let render_to_terminal = p.render_to_terminal;
    let echo_terminal_staged = render_to_terminal && p.out.is_none();

    let labels = StagedPlanRunLabels {
        planning_log_label: "逻辑双agent规划轮输出",
        step_injection_log_label: "逻辑双agent注入执行器user",
        build_planner_messages: build_logical_dual_planner_messages,
    };
    let req = prepare_staged_planner_no_tools_request(p, per_coord, labels.build_planner_messages)
        .await?;
    run_staged_plan_with_prepared_request(
        p,
        per_coord,
        req,
        render_to_terminal,
        echo_terminal_staged,
        labels,
        Message::user_only,
    )
    .await
}
