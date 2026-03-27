//! 分阶段规划与逻辑双 agent：规划轮 + 逐步注入执行。

use std::sync::atomic::Ordering;

use log::{debug, warn};
use tokio::sync::mpsc;

use crate::agent::per_coord::PerCoordinator;
use crate::llm::{complete_chat_retrying, no_tools_chat_request_from_messages};
use crate::sse::{SseErrorBody, SsePayload, encode_message};
use crate::types::{
    Message, USER_CANCELLED_FINISH_REASON, is_chat_ui_separator,
    message_clone_stripping_reasoning_for_api,
};

use super::execute_tools::sse_sender_closed;
use super::messages::push_assistant_merging_trailing_empty_placeholder;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;
use super::staged_sse::{
    emit_chat_ui_separator_sse, next_staged_plan_id, send_staged_plan_finished,
    send_staged_plan_notice, send_staged_plan_started, send_staged_plan_step_finished,
    send_staged_plan_step_started, staged_plan_phase_instruction_default,
    staged_plan_queue_summary_text,
};

/// 分阶段规划共享执行路径上的日志文案与 SSE 错误提示（避免 `run_staged_plan_with_prepared_request` 参数过长）。
pub(crate) struct StagedPlanRunLabels {
    pub planning_log_label: &'static str,
    pub tool_calls_error_message: &'static str,
    pub step_injection_log_label: &'static str,
}

async fn prepare_staged_planner_no_tools_request(
    p: &mut RunLoopParams<'_>,
    build_planner_messages: fn(&[Message], String) -> Vec<Message>,
) -> Result<crate::types::ChatRequest, Box<dyn std::error::Error + Send + Sync>> {
    crate::agent::context_window::prepare_messages_for_model(
        p.llm_backend,
        p.client,
        p.api_key,
        p.cfg.as_ref(),
        p.messages,
    )
    .await?;

    let instr = p.cfg.staged_plan_phase_instruction.trim();
    let plan_system = if instr.is_empty() {
        staged_plan_phase_instruction_default()
    } else {
        instr.to_string()
    };
    Ok(no_tools_chat_request_from_messages(
        p.cfg.as_ref(),
        build_planner_messages(p.messages, plan_system),
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

    let req =
        prepare_staged_planner_no_tools_request(p, build_single_agent_planner_messages).await?;
    run_staged_plan_with_prepared_request(
        p,
        per_coord,
        req,
        render_to_terminal,
        echo_terminal_staged,
        StagedPlanRunLabels {
            planning_log_label: "分阶段规划轮模型输出",
            tool_calls_error_message: "规划轮不应调用工具；请关闭 staged_plan_execution 或重试。",
            step_injection_log_label: "分步注入 user（完整正文，供排障与日志）",
        },
        |body| Message {
            role: "user".to_string(),
            content: Some(body),
            reasoning_content: None,
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
) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
        .map(message_clone_stripping_reasoning_for_api)
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

pub(crate) fn build_logical_dual_planner_messages(
    messages: &[Message],
    plan_system: String,
) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
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
        .map(message_clone_stripping_reasoning_for_api)
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
    let (msg, finish_reason) = complete_chat_retrying(
        p.llm_backend,
        p.client,
        p.api_key,
        p.cfg.as_ref(),
        &req,
        p.out,
        render_to_terminal,
        p.no_stream,
        p.cancel,
        p.plain_terminal_stream,
    )
    .await?;

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

    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        if let Some(tx) = p.out {
            let _ = tx
                .send(encode_message(SsePayload::Error(SseErrorBody {
                    error: labels.tool_calls_error_message.to_string(),
                    code: Some("staged_plan_tool_calls".to_string()),
                })))
                .await;
        }
        return Ok(());
    }

    let content = msg.content.as_deref().unwrap_or("");
    let plan = match crate::agent::plan_artifact::parse_agent_reply_plan_v1(content) {
        Ok(plan_v1) => plan_v1,
        Err(parse_err) => {
            let detail = crate::agent::plan_artifact::plan_artifact_error_log_summary(&parse_err);
            warn!(
                target: "crabmate",
                "staged_plan_invalid parse_err={} content_len={} content_preview={}",
                detail,
                content.chars().count(),
                crate::redact::preview_chars(content, crate::redact::MESSAGE_LOG_PREVIEW_CHARS)
            );
            if let Some(tx) = p.out {
                let _ = tx
                    .send(encode_message(SsePayload::Error(SseErrorBody {
                        error: "规划轮未解析出合法的 agent_reply_plan v1（需 ```json 围栏或单对象 JSON）。"
                            .to_string(),
                        code: Some("staged_plan_invalid".to_string()),
                    })))
                    .await;
            }
            return Err(
                crate::agent::plan_artifact::staged_plan_invalid_run_agent_turn_error(parse_err)
                    .into(),
            );
        }
    };

    push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());

    let plan_id = next_staged_plan_id();
    let n = plan.steps.len();
    send_staged_plan_started(p.out, &plan_id, n).await;

    send_staged_plan_notice(
        p.out,
        echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&plan, 0),
    )
    .await;

    let mut staged_loop_cancelled = false;
    let mut completed_steps = 0usize;
    for (i, step) in plan.steps.iter().enumerate() {
        if sse_sender_closed(p.out) || p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            staged_loop_cancelled = true;
            break;
        }
        let step_index = i + 1;
        send_staged_plan_step_started(
            p.out,
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
        let body = format!(
            "【分步执行 {}/{}】{}{}\n- id: {}\n- 描述: {}",
            step_index,
            n,
            crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE,
            summary_hint,
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
        p.messages.push(make_step_user_message(body));
        let run_step = run_agent_outer_loop(p, per_coord).await;
        if let Err(e) = run_step {
            finish_staged_plan_step_failed_and_plan_failed_sse(StagedPlanStepFailedExit {
                out: p.out,
                plan_id: &plan_id,
                step_id_trim: step.id.trim(),
                step_index,
                n,
                completed_steps_before_this: completed_steps,
            })
            .await;
            return Err(e);
        }
        if sse_sender_closed(p.out) || p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            finish_staged_plan_step_sse(
                p.out,
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
        finish_staged_plan_step_sse(p.out, &plan_id, step.id.trim(), step_index, n, "ok").await;
        completed_steps = step_index;
        p.messages.push(Message::chat_ui_separator(true));
        // 先发队列摘要 SSE，再追加 UI 分隔。
        send_staged_plan_notice(
            p.out,
            echo_terminal_staged,
            true,
            staged_plan_queue_summary_text(&plan, step_index),
        )
        .await;
        emit_chat_ui_separator_sse(p.out, true).await;
    }
    // 末步成功后循环内已发送含「[✓] 全部完成」的摘要，勿再发一次（否则重复一条）。
    send_staged_plan_finished(
        p.out,
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
        p.messages.push(Message::chat_ui_separator(true));
        emit_chat_ui_separator_sse(p.out, true).await;
    }
    Ok(())
}

pub(super) async fn run_logical_dual_agent_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let render_to_terminal = p.render_to_terminal;
    let echo_terminal_staged = render_to_terminal && p.out.is_none();

    let req =
        prepare_staged_planner_no_tools_request(p, build_logical_dual_planner_messages).await?;
    run_staged_plan_with_prepared_request(
        p,
        per_coord,
        req,
        render_to_terminal,
        echo_terminal_staged,
        StagedPlanRunLabels {
            planning_log_label: "逻辑双agent规划轮输出",
            tool_calls_error_message: "规划轮不应调用工具；请检查 planner_executor_mode 配置或重试。",
            step_injection_log_label: "逻辑双agent注入执行器user",
        },
        Message::user_only,
    )
    .await
}
