//! 分层多 Agent 执行入口
//!
//! 当 `planner_executor_mode = Hierarchical` 时使用此模块执行任务分解和子目标执行。

use super::hierarchical_intent_route::HierarchicalPostIntentRoute;
use super::orchestration_entry::{
    HierarchicalTurnEntryResolution, TurnOrchestrationTransition, log_orchestration_transition,
};
use crate::agent::hierarchy::{self, HierarchyRunnerResult};
use crate::agent::per_coord::PerCoordinator;
use crate::sse;

use super::errors::RunAgentTurnError;
use super::errors::{AgentTurnSubPhase, TurnAbortReason};
use super::intent_at_turn_start;
use super::intent_user;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;
use super::task_level_evidence::{
    is_program_build_run_request, render_task_level_evidence, verify_task_level_execution_evidence,
};
use super::turn_orchestration::TurnOrchestrationMode;
use crate::agent::hierarchy::execution_error::ExecutionError;
use crate::agent::hierarchy::turn_abort::HierarchicalTurnAbortReason;
use crate::agent::intent_pipeline::IntentAction;
use tracing::{info, warn};

/// `run_hierarchical_agent` 内子阶段（与顶层 [`TurnOrchestrationMode::Hierarchical`] 正交；供 `tracing` 字段 **`hierarchical_phase`**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HierarchicalRunPhase {
    /// 无有效 user 任务句，本路径返回 `Err`。
    NoUserTask,
    /// 分层意图门控（`run_intent_for_hierarchical`）。
    IntentGate,
    /// 意图门控已写入终答，本函数即将 `return Ok(())`。
    IntentGateFinishedEarly,
    /// 话语型 / 澄清确认 / 只读 QA：跳 Manager，走 **`run_agent_outer_loop`**。
    DiscourseFallbackOuter,
    /// Router → Manager → Operator → `run_hierarchical` 主路径。
    RouterManagerRunner,
    /// `run_hierarchical` 返回 `Err`：时间线 + 用户可见中止摘要。
    ExecutionAbortedSummary,
    /// 正常收尾：聚合子目标结果、任务级验收、终答气泡。
    FinalizeSummary,
}

impl HierarchicalRunPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::NoUserTask => "no_user_task",
            Self::IntentGate => "intent_gate",
            Self::IntentGateFinishedEarly => "intent_gate_finished_early",
            Self::DiscourseFallbackOuter => "discourse_fallback_outer",
            Self::RouterManagerRunner => "router_manager_runner",
            Self::ExecutionAbortedSummary => "execution_aborted_summary",
            Self::FinalizeSummary => "finalize_summary",
        }
    }
}

fn format_hierarchical_aborted_summary(e: &ExecutionError, task: &str) -> String {
    format!(
        "## 分层执行未正常结束\n\n**错误**：{e}\n\n**用户任务（摘要）**：{}\n\n若已出现 Manager 规划、子目标进度或工具结果，可结合上方时间线判断已完成的步骤。\n",
        truncate_string(task, 200)
    )
}

/// 将分层执行的总结正文写入 `messages` 与 SSE：`final_response` 时间线 + 终答相（与其它分层收尾路径一致）。
///
/// 顺序：先发带协议封装的 `timeline_log(kind=final_response)`，再发 `assistant_answer_phase`。
/// Web 端以 `on_timeline_log` 落主气泡；不再下发裸 Markdown delta，避免与 `timeline_log`
/// 重复以及由缓冲/解析顺序带来的总结缺失问题。
async fn emit_hierarchical_final_assistant(p: &mut RunLoopParams<'_>, final_response: String) {
    p.turn.push_message(crate::types::Message::assistant_only(
        final_response.clone(),
    ));
    if let Some(out) = p.ctx.io.out {
        crate::sse::send_final_response_timeline_then_answer_phase(
            out,
            final_response,
            "hierarchical::final_response",
            "hierarchical::answer_phase",
        )
        .await;
    }
}

/// 运行分层多 Agent
pub(crate) async fn run_hierarchical_agent(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    let in_clarification_flow =
        intent_user::recently_waiting_execute_confirmation(p.turn.messages());
    let task = intent_user::extract_effective_user_task(p.turn.messages(), in_clarification_flow);
    if task.is_empty() {
        warn!(
            target: "crabmate::agent_turn",
            turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
            hierarchical_phase = HierarchicalRunPhase::NoUserTask.as_str(),
            "run_hierarchical_agent: no user task"
        );
        log::warn!(target: "crabmate", "Hierarchical mode: no user task found");
        return Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: "Hierarchical mode requires a user message".to_string(),
        });
    }

    info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
        hierarchical_phase = HierarchicalRunPhase::IntentGate.as_str(),
        task_preview = %truncate_string(&task, 120),
        "run_hierarchical_agent intent_gate"
    );
    let intent_gate = intent_at_turn_start::run_intent_for_hierarchical(p, &task).await?;
    let assessment = match intent_gate {
        intent_at_turn_start::IntentGateResult::Finished => {
            info!(
                target: "crabmate::agent_turn",
                turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
                hierarchical_phase = HierarchicalRunPhase::IntentGateFinishedEarly.as_str(),
                "run_hierarchical_agent intent finished early"
            );
            return Ok(());
        }
        intent_at_turn_start::IntentGateResult::ProceedExecute { assessment } => assessment,
    };

    let entry = HierarchicalTurnEntryResolution::resolve(&assessment);
    let post_intent = entry.post_intent_route;
    log_orchestration_transition(
        TurnOrchestrationTransition::HierarchicalPostIntentResolved,
        Some(entry.orchestration_mode.as_str()),
        &[
            ("hierarchical_post_intent_route", post_intent.as_str()),
            (
                "hierarchical_discourse_fallback_reason",
                match post_intent {
                    HierarchicalPostIntentRoute::DiscourseFallbackOuter(r) => r.as_str(),
                    HierarchicalPostIntentRoute::RouterManagerRunner => "",
                },
            ),
        ],
    );
    match post_intent {
        HierarchicalPostIntentRoute::DiscourseFallbackOuter(reason) => {
            crate::turn_replay_dump::append_decision_point_event_if_configured(
                "intent",
                "agent_execution_mode",
                "single_agent_outer_loop",
                "意图判定为话语型/澄清确认流，跳过分层 Manager，转主模型单 Agent 外循环",
                serde_json::json!({
                    "intent_kind": format!("{:?}", assessment.kind),
                    "primary_intent": assessment.primary_intent,
                    "hierarchical_post_intent_route": post_intent.as_str(),
                    "hierarchical_discourse_fallback_reason": reason.as_str(),
                }),
                "current_turn",
                None,
            );
            let action_tag = match &assessment.action {
                IntentAction::Execute => "Execute",
                IntentAction::DirectReply(_) => "DirectReply",
                IntentAction::ClarifyThenExecute(_) => "ClarifyThenExecute",
                IntentAction::ConfirmThenExecute(_) => "ConfirmThenExecute",
            };
            log::info!(
                target: "crabmate",
                "[HIERARCHICAL] kind={:?} primary={} action={}: discourse/clarify/confirm delegates to main model; skipping Manager/decompose, using single-agent outer loop",
                assessment.kind,
                assessment.primary_intent,
                action_tag
            );
            info!(
                target: "crabmate::agent_turn",
                turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
                hierarchical_phase = HierarchicalRunPhase::DiscourseFallbackOuter.as_str(),
                hierarchical_post_intent_route = post_intent.as_str(),
                hierarchical_discourse_fallback_reason = reason.as_str(),
                intent_kind = ?assessment.kind,
                primary_intent = %assessment.primary_intent,
                action = action_tag,
                "run_hierarchical_agent discourse fallback to outer_loop"
            );
            return run_agent_outer_loop(p, per_coord).await;
        }
        HierarchicalPostIntentRoute::RouterManagerRunner => {}
    }

    log::info!(
        target: "crabmate",
        "[HIERARCHICAL] === Agent Role Enter === role=hierarchical task={}",
        truncate_string(&task, 100)
    );
    info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
        hierarchical_phase = HierarchicalRunPhase::RouterManagerRunner.as_str(),
        task_preview = %truncate_string(&task, 120),
        "run_hierarchical_agent router_manager_runner"
    );

    let params = p.hierarchy_runner_params(
        &task,
        Some(assessment.primary_intent.clone()),
        assessment.secondary_intents.clone(),
    );

    // 运行分层 Agent：失败时也输出总结性终答，避免主气泡无收尾
    let result = match hierarchy::runner::run_hierarchical(params).await {
        Ok(r) => r,
        Err(ExecutionError::TurnAborted(reason)) => {
            let abort_reason = match reason {
                HierarchicalTurnAbortReason::UserCancelled => TurnAbortReason::UserCancelled,
                HierarchicalTurnAbortReason::SseDisconnected => TurnAbortReason::SseDisconnected,
            };
            info!(
                target: "crabmate::agent_turn",
                turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
                hierarchical_phase = HierarchicalRunPhase::ExecutionAbortedSummary.as_str(),
                abort_reason = ?abort_reason,
                "run_hierarchical_agent turn aborted"
            );
            return Err(RunAgentTurnError::TurnAborted {
                phase: AgentTurnSubPhase::Executor,
                reason: abort_reason,
            });
        }
        Err(e) => {
            info!(
                target: "crabmate::agent_turn",
                turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
                hierarchical_phase = HierarchicalRunPhase::ExecutionAbortedSummary.as_str(),
                error = %e,
                "run_hierarchical_agent execution error"
            );
            log::error!(target: "crabmate", "Hierarchical agent failed: {}", e);
            if let Some(out) = p.ctx.io.out {
                let title = format!("分层执行未正常完成：{e}");
                let _ = sse::send_string_logged(
                    out,
                    sse::encode_message(crate::sse::SsePayload::TimelineLog {
                        log: crate::sse::protocol::TimelineLogBody {
                            kind: "hierarchical_execution".to_string(),
                            title,
                            detail: None,
                        },
                    }),
                    "hierarchical::execution_aborted_timeline",
                )
                .await;
            }
            let final_response = format_hierarchical_aborted_summary(&e, &task);
            emit_hierarchical_final_assistant(p, final_response).await;
            return Ok(());
        }
    };

    // 处理执行结果
    handle_execution_result(p, result, &task).await?;

    Ok(())
}

/// 处理分层执行结果
async fn handle_execution_result(
    p: &mut RunLoopParams<'_>,
    result: HierarchyRunnerResult,
    original_task: &str,
) -> Result<(), RunAgentTurnError> {
    let HierarchyRunnerResult {
        execution_result,
        mode,
    } = result;

    info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
        hierarchical_phase = HierarchicalRunPhase::FinalizeSummary.as_str(),
        router_mode = %mode.as_str(),
        total_completed = execution_result.total_completed,
        total_failed = execution_result.total_failed,
        total_duration_ms = execution_result.total_duration_ms,
        goals = execution_result.results.len(),
        "handle_execution_result finalize"
    );

    log::info!(
        target: "crabmate",
        "Hierarchical execution completed: mode={} completed={} failed={} duration={}ms",
        mode.as_str(),
        execution_result.total_completed,
        execution_result.total_failed,
        execution_result.total_duration_ms
    );

    // 发送执行摘要到 SSE
    if let Some(out) = p.ctx.io.out {
        let summary = format!(
            "分层执行完成：模式={}, 完成={}, 失败={}, 耗时={}ms",
            mode.as_str(),
            execution_result.total_completed,
            execution_result.total_failed,
            execution_result.total_duration_ms
        );

        let _ = sse::send_string_logged(
            out,
            sse::encode_message(crate::sse::SsePayload::TimelineLog {
                log: crate::sse::protocol::TimelineLogBody {
                    kind: "hierarchical_execution".to_string(),
                    title: summary,
                    detail: None,
                },
            }),
            "hierarchical::execution_summary",
        )
        .await;
    }

    // 如果有失败的任务，记录警告
    if execution_result.total_failed > 0 {
        log::warn!(
            target: "crabmate",
            "Hierarchical execution had {} failed sub-goals",
            execution_result.total_failed
        );
    }

    // 终答：始终含「概览 + 计划完成态 + 子目标精简表」；再追加任务级验收或全类任务小结
    let body = aggregate_results(&execution_result.results);
    let plan_done = render_plan_completion(&execution_result.results);
    let intro = format!(
        "**分层执行概览**（模式 `{}`）：子目标 **{}** 个；其中完成 **{}**、失败 **{}**；总耗时约 **{}** ms。\n\n{}\n\n{}\n\n",
        mode.as_str(),
        execution_result.results.len(),
        execution_result.total_completed,
        execution_result.total_failed,
        execution_result.total_duration_ms,
        if execution_result.total_failed > 0 {
            "子目标层存在失败项，详见下表带 ❌ 的条目；若下节「任务级验收」有说明，以验收为准。"
        } else {
            "子目标层无失败状态返回，详见下表。"
        },
        plan_done
    );
    let with_core = format!("{intro}{body}");
    let evidence_md = render_task_level_evidence(
        original_task,
        &execution_result.results,
        &execution_result.goal_expected_outputs,
    );
    let with_evidence = if evidence_md.is_empty() {
        with_core
    } else {
        format!("{with_core}\n\n{evidence_md}")
    };

    let task_acceptance_reason = verify_task_level_execution_evidence(
        original_task,
        &execution_result.results,
        &execution_result.goal_expected_outputs,
    );
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "acceptance_check",
        "task_level",
        Some(&serde_json::json!({
            "passed": task_acceptance_reason.is_none(),
            "reason": task_acceptance_reason.clone(),
            "total_goals": execution_result.results.len(),
            "total_completed": execution_result.total_completed,
            "total_failed": execution_result.total_failed,
            "duration_ms": execution_result.total_duration_ms,
            "phase": "acceptance",
        })),
    );
    crate::turn_replay_dump::append_decision_point_event_if_configured(
        "acceptance",
        "task_acceptance",
        if task_acceptance_reason.is_none() {
            "pass"
        } else {
            "fail"
        },
        if task_acceptance_reason.is_none() {
            "任务级验收通过"
        } else {
            "任务级验收未通过"
        },
        serde_json::json!({
            "reason": task_acceptance_reason.clone(),
            "total_goals": execution_result.results.len(),
            "total_completed": execution_result.total_completed,
            "total_failed": execution_result.total_failed,
            "duration_ms": execution_result.total_duration_ms,
        }),
        "current_turn",
        None,
    );
    let final_response: String = if let Some(reason) = task_acceptance_reason {
        format!("{with_evidence}\n\n---\n**任务级验收（未通过）**：{reason}\n")
    } else if is_program_build_run_request(original_task) {
        if execution_result.total_failed == 0 {
            format!(
                "{with_evidence}\n\n---\n**任务级验收**：已通过（写源码/编译/运行等要求可在子目标与工具结果中核对）。\n"
            )
        } else {
            format!(
                "{with_evidence}\n\n---\n**任务级验收**：因存在未成功的子目标，不记为整任务通过，请据上表排查。\n"
            )
        }
    } else if execution_result.total_failed > 0 {
        format!(
            "{with_evidence}\n\n---\n**小结**：本轮有 **{}** 个子目标未成功，请据上表排查。\n",
            execution_result.total_failed
        )
    } else {
        format!(
            "{with_evidence}\n\n---\n**小结**：本轮子目标均成功，可视为在分层阶段已满足执行侧结论。\n"
        )
    };

    emit_hierarchical_final_assistant(p, final_response).await;

    Ok(())
}

/// 汇总子目标结果生成最终回复
fn aggregate_results(results: &[crate::agent::hierarchy::TaskResult]) -> String {
    if results.is_empty() {
        return "任务已完成".to_string();
    }

    let mut lines = Vec::new();
    lines.push("## 分层执行结果（精简）\n".to_string());

    for result in results {
        match &result.status {
            crate::agent::hierarchy::TaskStatus::Completed => {
                lines.push(format!(
                    "- ✅ 完成: {} ({}ms)",
                    result.task_id, result.duration_ms
                ));
            }
            crate::agent::hierarchy::TaskStatus::Failed { reason } => {
                lines.push(format!(
                    "- ❌ 失败: {} ({}ms) - {}",
                    result.task_id, result.duration_ms, reason
                ));
            }
            crate::agent::hierarchy::TaskStatus::Pending => {
                lines.push(format!("- ⏳ 进行中: {}", result.task_id));
                continue;
            }
            crate::agent::hierarchy::TaskStatus::InProgress => {
                lines.push(format!("- 🔄 进行中: {}", result.task_id));
                continue;
            }
            crate::agent::hierarchy::TaskStatus::Skipped { .. } => {
                lines.push(format!("- ⏭️ 跳过: {}", result.task_id));
                continue;
            }
            crate::agent::hierarchy::TaskStatus::NeedsDecomposition {
                suggested_subgoals, ..
            } => {
                lines.push(format!(
                    "- 🔄 需要分解: {} (建议 {} 个子目标)",
                    result.task_id, suggested_subgoals
                ));
                continue;
            }
        };
    }

    lines.join("\n")
}

fn render_plan_completion(results: &[crate::agent::hierarchy::TaskResult]) -> String {
    if results.is_empty() {
        return String::new();
    }
    let mut lines = vec!["## 计划完成态".to_string(), String::new()];
    for r in results {
        match &r.status {
            crate::agent::hierarchy::TaskStatus::Completed => {
                lines.push(format!("- [x] {}（{}ms）", r.task_id, r.duration_ms));
            }
            crate::agent::hierarchy::TaskStatus::Failed { reason } => {
                lines.push(format!(
                    "- [!] {}（{}ms）：{}",
                    r.task_id, r.duration_ms, reason
                ));
            }
            crate::agent::hierarchy::TaskStatus::Skipped { reason } => {
                lines.push(format!("- [-] {}：{}", r.task_id, reason));
            }
            crate::agent::hierarchy::TaskStatus::NeedsDecomposition {
                reason,
                suggested_subgoals,
            } => {
                lines.push(format!(
                    "- [~] {}：{}（建议分解 {}）",
                    r.task_id, reason, suggested_subgoals
                ));
            }
            crate::agent::hierarchy::TaskStatus::Pending
            | crate::agent::hierarchy::TaskStatus::InProgress => {
                lines.push(format!("- [ ] {}", r.task_id));
            }
        }
    }
    lines.join("\n")
}

/// 截断字符串（按字符边界截断，支持中文）
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated = s
            .char_indices()
            .take(max_len.saturating_sub(3))
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &s[..truncated])
    }
}

#[cfg(test)]
mod tests {
    use super::super::intent;
    use crate::agent::intent_pipeline::{IntentAction, IntentDecision};
    use crate::agent::intent_router::IntentKind;
    use crate::types::Message;

    #[test]
    fn hierarchical_run_phase_as_str_stable() {
        use super::HierarchicalRunPhase;
        assert_eq!(HierarchicalRunPhase::NoUserTask.as_str(), "no_user_task");
        assert_eq!(
            HierarchicalRunPhase::DiscourseFallbackOuter.as_str(),
            "discourse_fallback_outer"
        );
        assert_eq!(
            HierarchicalRunPhase::RouterManagerRunner.as_str(),
            "router_manager_runner"
        );
    }

    #[test]
    fn confirmation_followup_uses_previous_user_task() {
        let messages = vec![
            Message::user_only("编写一个简单c++程序并执行".to_string()),
            Message::assistant_only(
                "我判断你可能想让我直接执行任务。请确认是否“直接开始执行”，或补充更具体范围。"
                    .to_string(),
            ),
            Message::user_only("直接开始执行".to_string()),
        ];
        let task = intent::intent_user::extract_effective_user_task(&messages, true);
        assert_eq!(task, "编写一个简单c++程序并执行");
    }

    #[test]
    fn normal_latest_user_task_kept_when_not_confirmation() {
        let messages = vec![
            Message::user_only("先看看目录".to_string()),
            Message::assistant_only("好的".to_string()),
            Message::user_only("编写一个简单c++程序并执行".to_string()),
        ];
        let task = intent::intent_user::extract_effective_user_task(&messages, false);
        assert_eq!(task, "编写一个简单c++程序并执行");
    }

    #[test]
    fn intent_analysis_title_includes_primary_and_action() {
        let assessment = IntentDecision {
            kind: IntentKind::Execute,
            confidence: 0.61,
            action: IntentAction::Execute,
            primary_intent: "execute.code_change".to_string(),
            secondary_intents: Vec::new(),
            abstain: false,
            need_clarification: false,
        };
        let title = format_intent_title(&assessment);
        assert!(title.contains("kind=Execute"));
        assert!(title.contains("primary=execute.code_change"));
        assert!(title.contains("action=直接执行"));
    }

    fn format_intent_title(assessment: &IntentDecision) -> String {
        use crate::agent::intent_pipeline::IntentAction;
        let action = match &assessment.action {
            IntentAction::Execute => "直接执行",
            IntentAction::ConfirmThenExecute(_) => "确认后执行",
            IntentAction::ClarifyThenExecute(_) => "澄清后执行",
            IntentAction::DirectReply(_) => "直接回复",
        };
        format!(
            "意图分析：kind={:?}, primary={}, action={}",
            assessment.kind, assessment.primary_intent, action
        )
    }
}
