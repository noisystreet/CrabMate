//! 分层多 Agent 执行入口
//!
//! 当 `planner_executor_mode = Hierarchical` 时使用此模块执行任务分解和子目标执行。

use crate::agent::hierarchy::task::{ArtifactKind, BuildArtifactKind, TaskResult};
use crate::agent::hierarchy::{self, HierarchyRunnerResult};
use crate::agent::intent_router::{
    IntentKind, intent_reply_delegates_to_main_model, qa_readonly_style_primary,
};
use crate::agent::per_coord::{PerCoordinator, PerCoordinatorInit};
use crate::sse;
use std::collections::HashMap;

use super::errors::RunAgentTurnError;
use super::intent_at_turn_start;
use super::intent_user;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;
use crate::agent::agent_turn::errors::AgentTurnSubPhase;
use crate::agent::hierarchy::execution::ExecutionError;
use crate::agent::intent_pipeline::IntentAction;

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
    p.messages.push(crate::types::Message::assistant_only(
        final_response.clone(),
    ));
    if let Some(out) = p.out {
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
) -> Result<(), RunAgentTurnError> {
    let in_clarification_flow = intent_user::recently_waiting_execute_confirmation(p.messages);
    let task = intent_user::extract_effective_user_task(p.messages, in_clarification_flow);
    if task.is_empty() {
        log::warn!(target: "crabmate", "Hierarchical mode: no user task found");
        return Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: "Hierarchical mode requires a user message".to_string(),
        });
    }

    let intent_gate = intent_at_turn_start::run_intent_for_hierarchical(p, &task).await?;
    let assessment = match intent_gate {
        intent_at_turn_start::IntentGateResult::Finished => {
            return Ok(());
        }
        intent_at_turn_start::IntentGateResult::ProceedExecute { assessment } => assessment,
    };

    let skip_manager_for_discourse =
        intent_reply_delegates_to_main_model(assessment.kind, &assessment.primary_intent)
            || matches!(
                &assessment.action,
                IntentAction::ClarifyThenExecute(_) | IntentAction::ConfirmThenExecute(_)
            )
            || (assessment.kind == IntentKind::Qa
                && qa_readonly_style_primary(&assessment.primary_intent)
                && matches!(&assessment.action, IntentAction::DirectReply(_)));
    if skip_manager_for_discourse {
        crate::turn_replay_dump::append_decision_point_event_if_configured(
            "intent",
            "agent_execution_mode",
            "single_agent_outer_loop",
            "意图判定为话语型/澄清确认流，跳过分层 Manager，转主模型单 Agent 外循环",
            serde_json::json!({
                "intent_kind": format!("{:?}", assessment.kind),
                "primary_intent": assessment.primary_intent,
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
        let mut per_coord =
            PerCoordinator::new(PerCoordinatorInit::from_agent_config(p.cfg.as_ref()));
        return run_agent_outer_loop(p, &mut per_coord).await;
    }

    log::info!(
        target: "crabmate",
        "[HIERARCHICAL] === Agent Role Enter === role=hierarchical task={}",
        truncate_string(&task, 100)
    );

    let params = p.hierarchy_runner_params(
        &task,
        Some(assessment.primary_intent.clone()),
        assessment.secondary_intents.clone(),
    );

    // 运行分层 Agent：失败时也输出总结性终答，避免主气泡无收尾
    let result = match hierarchy::runner::run_hierarchical(params).await {
        Ok(r) => r,
        Err(e) => {
            log::error!(target: "crabmate", "Hierarchical agent failed: {}", e);
            if let Some(out) = p.out {
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

    log::info!(
        target: "crabmate",
        "Hierarchical execution completed: mode={} completed={} failed={} duration={}ms",
        mode.as_str(),
        execution_result.total_completed,
        execution_result.total_failed,
        execution_result.total_duration_ms
    );

    // 发送执行摘要到 SSE
    if let Some(out) = p.out {
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

fn is_program_build_run_request(task: &str) -> bool {
    let t = task.to_lowercase();
    let asks_write = t.contains("编写") || t.contains("实现") || t.contains("write");
    let asks_program = t.contains("程序") || t.contains("c++") || t.contains("cpp");
    let asks_run = t.contains("执行")
        || t.contains("运行")
        || t.contains("编译")
        || t.contains("build")
        || t.contains("run");
    asks_write && asks_program && asks_run
}

fn verify_task_level_execution_evidence(
    task: &str,
    results: &[TaskResult],
    goal_expected_outputs: &HashMap<String, Vec<String>>,
) -> Option<String> {
    if !is_program_build_run_request(task) {
        return None;
    }
    let mut wrote_source = false;
    let mut compiled = false;
    let mut ran_program = false;
    let expected_outputs = expected_output_hints_for_results(task, results, goal_expected_outputs);

    for r in results {
        let combined = format!(
            "{}\n{}",
            r.output.as_deref().unwrap_or(""),
            r.error.as_deref().unwrap_or("")
        )
        .to_lowercase();
        for a in &r.artifacts {
            match a.kind {
                ArtifactKind::File => {
                    if a.path.as_deref().is_some_and(|p| {
                        let p = p.to_lowercase();
                        p.ends_with(".cpp") || p.ends_with(".cc") || p.ends_with(".cxx")
                    }) {
                        wrote_source = true;
                    }
                }
                ArtifactKind::BuildArtifact(kind) => match kind {
                    BuildArtifactKind::SourceFile => wrote_source = true,
                    BuildArtifactKind::ObjectFile => compiled = true,
                    _ => {}
                },
                _ => {}
            }
        }
        let combined_full = format!(
            "{}\n{}",
            r.output.as_deref().unwrap_or(""),
            r.error.as_deref().unwrap_or("")
        );
        if r.tools_invoked.iter().any(|n| n == "run_executable")
            || (r.tools_invoked.iter().any(|n| n == "run_command")
                && crate::agent::hierarchy::goal_verifier::run_command_invocation_matches_expected_output(
                    &combined_full,
                    &expected_outputs,
                ))
        {
            ran_program = true;
        }
        if combined.contains("create_file")
            || combined.contains("已创建文件")
            || combined.contains("created file")
            || combined.contains("write_file")
            || combined.contains("apply_patch")
            || combined.contains(".cpp")
        {
            wrote_source = true;
        }
        if combined.contains("g++")
            || combined.contains("clang++")
            || combined.contains("编译")
            || combined.contains("cmake")
            || combined.contains("make")
            || combined.contains("build")
        {
            compiled = true;
        }
    }

    let mut missing = Vec::new();
    if !wrote_source {
        missing.push("write_source");
    }
    if !compiled {
        missing.push("compile");
    }
    if !ran_program {
        missing.push("run");
    }
    if missing.is_empty() {
        None
    } else {
        Some(format!(
            "missing: {}; 需要至少包含写源码(.cpp)+编译(g++/clang++)+运行(可执行输出)",
            missing.join(",")
        ))
    }
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

fn render_task_level_evidence(
    task: &str,
    results: &[crate::agent::hierarchy::TaskResult],
    goal_expected_outputs: &HashMap<String, Vec<String>>,
) -> String {
    if !is_program_build_run_request(task) {
        return String::new();
    }

    let mut wrote_source = false;
    let mut built_binary = false;
    let mut ran_binary = false;
    let mut seen_expected_output = false;
    let expected_outputs = expected_output_hints_for_results(task, results, goal_expected_outputs);
    let mut matched_expected_outputs: Vec<String> = Vec::new();

    for r in results {
        let combined = format!(
            "{}\n{}",
            r.output.as_deref().unwrap_or(""),
            r.error.as_deref().unwrap_or("")
        );
        let lower = combined.to_lowercase();
        for a in &r.artifacts {
            if a.path.as_deref().is_some_and(|p| {
                let p = p.to_lowercase();
                p.ends_with(".cpp") || p.ends_with(".cc") || p.ends_with(".cxx")
            }) {
                wrote_source = true;
            }
            if a.path
                .as_deref()
                .is_some_and(|p| p.to_lowercase().contains("build/"))
            {
                built_binary = true;
            }
        }
        if lower.contains("built target")
            || lower.contains("cmake --build")
            || lower.contains("linking cxx executable")
        {
            built_binary = true;
        }
        if r.tools_invoked.iter().any(|n| n == "run_executable")
            || r.tools_invoked.iter().any(|n| n == "run_command")
                && crate::agent::hierarchy::goal_verifier::run_command_invocation_matches_expected_output(
                    &combined,
                    &expected_outputs,
                )
        {
            ran_binary = true;
        }
        for hint in &expected_outputs {
            if hint.is_empty() {
                continue;
            }
            if lower.contains(&hint.to_lowercase()) {
                seen_expected_output = true;
                if !matched_expected_outputs
                    .iter()
                    .any(|x| x.eq_ignore_ascii_case(hint))
                {
                    matched_expected_outputs.push(hint.clone());
                }
            }
        }
    }

    let mut lines = vec!["## 关键证据".to_string(), String::new()];
    lines.push(format!(
        "- 源码落地：{}",
        if wrote_source {
            "已检测到 `.cpp` 源文件写入"
        } else {
            "未检测到明确证据"
        }
    ));
    lines.push(format!(
        "- 编译产物：{}",
        if built_binary {
            "已检测到构建/链接成功信号"
        } else {
            "未检测到明确证据"
        }
    ));
    lines.push(format!(
        "- 运行验证：{}",
        if ran_binary || seen_expected_output {
            if expected_outputs.is_empty() {
                "已检测到程序执行（含可核对输出）"
            } else {
                "已检测到程序执行（含期望输出）"
            }
        } else {
            "未检测到明确证据"
        }
    ));
    if !expected_outputs.is_empty() {
        let expected_joined = expected_outputs
            .iter()
            .map(|s| format!("`{}`", s))
            .collect::<Vec<_>>()
            .join("、");
        lines.push(format!("- acceptance 期望输出：{}", expected_joined));
        if matched_expected_outputs.is_empty() {
            lines.push("- acceptance 核对结果：未在工具输出中检测到期望片段".to_string());
        } else {
            let matched_joined = matched_expected_outputs
                .iter()
                .map(|s| format!("`{}`", s))
                .collect::<Vec<_>>()
                .join("、");
            lines.push(format!(
                "- acceptance 核对结果：已检测到 {}",
                matched_joined
            ));
        }
    }
    lines.join("\n")
}

fn expected_output_hints_from_task(task: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(re) = regex::Regex::new(r#""([^"\n]{1,120})""#) {
        for cap in re.captures_iter(task) {
            if let Some(m) = cap.get(1) {
                let t = m.as_str().trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
            }
        }
    }
    if out.is_empty() && task.to_lowercase().contains("hello") {
        out.push("hello".to_string());
    }
    out
}

fn expected_output_hints_for_results(
    task: &str,
    results: &[TaskResult],
    goal_expected_outputs: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in results {
        if let Some(v) = goal_expected_outputs.get(&r.task_id) {
            for s in v {
                let t = s.trim();
                if !t.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                    out.push(t.to_string());
                }
            }
        }
    }
    if out.is_empty() {
        return expected_output_hints_from_task(task);
    }
    out
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
