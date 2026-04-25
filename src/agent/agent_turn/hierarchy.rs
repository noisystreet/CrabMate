//! 分层多 Agent 执行入口
//!
//! 当 `planner_executor_mode = Hierarchical` 时使用此模块执行任务分解和子目标执行。

use crate::agent::hierarchy::task::{ArtifactKind, BuildArtifactKind, TaskResult};
use crate::agent::hierarchy::{self, HierarchyRunnerParams, HierarchyRunnerResult};
use crate::sse;

use super::errors::RunAgentTurnError;
use super::intent_at_turn_start;
use super::intent_user;
use super::params::RunLoopParams;
use crate::agent::agent_turn::errors::AgentTurnSubPhase;
use crate::agent::hierarchy::execution::ExecutionError;

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
        let final_tl = sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "final_response".to_string(),
                title: final_response,
                detail: None,
            },
        });
        let _ = sse::send_string_logged(out, final_tl, "hierarchical::final_response").await;
        let phase_payload = sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
            assistant_answer_phase: true,
        });
        let _ = sse::send_string_logged(out, phase_payload, "hierarchical::answer_phase").await;
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

    log::info!(
        target: "crabmate",
        "[HIERARCHICAL] === Agent Role Enter === role=hierarchical task={}",
        truncate_string(&task, 100)
    );

    // 构建运行参数
    // 从 web_tool_ctx 中提取审批上下文（如果存在）
    let (tool_approval_out, tool_approval_rx) = if let Some(web_ctx) = p.web_tool_ctx {
        (
            Some(web_ctx.out_tx.clone()),
            Some(web_ctx.approval_rx_shared.clone()),
        )
    } else {
        (None, None)
    };

    let params = HierarchyRunnerParams {
        task: &task,
        cfg: p.cfg.as_ref(),
        llm_backend: p.llm_backend,
        client: std::sync::Arc::new(p.client.clone()),
        api_key: p.api_key.to_string(),
        working_dir: p.effective_working_dir.to_path_buf(),
        sse_out: p.out.cloned(),
        tools_defs: p.tools_defs,
        tool_approval_out,
        tool_approval_rx,
        primary_intent: Some(assessment.primary_intent.clone()),
        secondary_intents: assessment.secondary_intents.clone(),
        intent_mode_bias_enabled: p.cfg.intent_mode_bias_enabled,
    };

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

    // 终答：始终含「概览 + 子目标表」；再追加任务级验收或全类任务小结
    let body = aggregate_results(&execution_result.results);
    let intro = format!(
        "**分层执行概览**（模式 `{}`）：子目标 **{}** 个；其中完成 **{}**、失败 **{}**；总耗时约 **{}** ms。\n\n{}\n\n",
        mode.as_str(),
        execution_result.results.len(),
        execution_result.total_completed,
        execution_result.total_failed,
        execution_result.total_duration_ms,
        if execution_result.total_failed > 0 {
            "子目标层存在失败项，详见下表带 ❌ 的条目；若下节「任务级验收」有说明，以验收为准。"
        } else {
            "子目标层无失败状态返回，详见下表。"
        }
    );
    let with_core = format!("{intro}{body}");

    let final_response: String = if let Some(reason) =
        verify_task_level_execution_evidence(original_task, &execution_result.results)
    {
        format!("{with_core}\n\n---\n**任务级验收（未通过）**：{reason}\n")
    } else if is_program_build_run_request(original_task) {
        if execution_result.total_failed == 0 {
            format!(
                "{with_core}\n\n---\n**任务级验收**：已通过（写源码/编译/运行等要求可在子目标与工具结果中核对）。\n"
            )
        } else {
            format!(
                "{with_core}\n\n---\n**任务级验收**：因存在未成功的子目标，不记为整任务通过，请据上表排查。\n"
            )
        }
    } else if execution_result.total_failed > 0 {
        format!(
            "{with_core}\n\n---\n**小结**：本轮有 **{}** 个子目标未成功，请据上表与工具输出排查。\n",
            execution_result.total_failed
        )
    } else {
        format!(
            "{with_core}\n\n---\n**小结**：本轮子目标均成功，可视为在分层阶段已满足执行侧结论。\n"
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

fn verify_task_level_execution_evidence(task: &str, results: &[TaskResult]) -> Option<String> {
    if !is_program_build_run_request(task) {
        return None;
    }
    let mut wrote_source = false;
    let mut compiled = false;
    let mut ran_program = false;

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
                && crate::agent::hierarchy::goal_verifier::run_command_invocation_mentions_hello(
                    &combined_full,
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
    lines.push("## 分层执行结果\n".to_string());

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

        // 显示任务输出（包括成功和失败的任务）
        if let Some(output) = &result.output {
            lines.push(format!("  {}", output));
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
    use super::super::intent_user;
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
        let task = intent_user::extract_effective_user_task(&messages, true);
        assert_eq!(task, "编写一个简单c++程序并执行");
    }

    #[test]
    fn normal_latest_user_task_kept_when_not_confirmation() {
        let messages = vec![
            Message::user_only("先看看目录".to_string()),
            Message::assistant_only("好的".to_string()),
            Message::user_only("编写一个简单c++程序并执行".to_string()),
        ];
        let task = intent_user::extract_effective_user_task(&messages, false);
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
