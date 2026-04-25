//! 分层多 Agent 执行入口
//!
//! 当 `planner_executor_mode = Hierarchical` 时使用此模块执行任务分解和子目标执行。

use crate::agent::hierarchy::task::{ArtifactKind, BuildArtifactKind, TaskResult};
use crate::agent::hierarchy::{self, HierarchyRunnerParams, HierarchyRunnerResult};
use crate::agent::intent_l2_classifier::classify_intent_l2_with_llm;
use crate::agent::intent_pipeline::{
    IntentAction, IntentContext, IntentDecision, assess_and_route_with_l2,
};
use crate::agent::intent_router::ExecuteIntentThresholds;
use crate::agent::intent_router::{
    is_explicit_execute_confirmation, is_waiting_execute_confirmation_prompt,
};
use crate::sse;

use super::errors::RunAgentTurnError;
use super::params::RunLoopParams;
use crate::agent::agent_turn::errors::AgentTurnSubPhase;
use crate::agent::hierarchy::execution::ExecutionError;

fn format_hierarchical_aborted_summary(e: &ExecutionError, task: &str) -> String {
    format!(
        "## 分层执行未正常结束\n\n**错误**：{e}\n\n**用户任务（摘要）**：{}\n\n若已出现 Manager 规划、子目标进度或工具结果，可结合上方时间线判断已完成的步骤。\n",
        truncate_string(task, 200)
    )
}

/// 将分层执行的总结正文写入 `messages` 与 SSE：终答相 + 正文 delta + `final_response` 时间线（与其它分层收尾路径一致）。
async fn emit_hierarchical_final_assistant(p: &mut RunLoopParams<'_>, final_response: String) {
    p.messages.push(crate::types::Message::assistant_only(
        final_response.clone(),
    ));
    if let Some(out) = p.out {
        let phase_payload = sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
            assistant_answer_phase: true,
        });
        let _ = sse::send_string_logged(out, phase_payload, "hierarchical::answer_phase").await;
        let _ = sse::send_string_logged(
            out,
            final_response.clone(),
            "hierarchical::final_response_delta",
        )
        .await;
        let final_tl = sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "final_response".to_string(),
                title: final_response,
                detail: None,
            },
        });
        let _ = sse::send_string_logged(out, final_tl, "hierarchical::final_response").await;
    }
}

fn recently_waiting_execute_confirmation(messages: &[crate::types::Message]) -> bool {
    messages.iter().rev().take(4).any(|m| {
        if m.role != "assistant" {
            return false;
        }
        let Some(content) = crate::types::message_content_as_str(&m.content) else {
            return false;
        };
        is_waiting_execute_confirmation_prompt(content)
    })
}

fn format_intent_analysis_title(assessment: &IntentDecision) -> String {
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

fn format_intent_analysis_detail(
    assessment: &IntentDecision,
    merge_meta: &crate::agent::intent_pipeline::IntentMergeMeta,
) -> String {
    let l0 = &merge_meta.l0;
    format!(
        "confidence={:.2}, need_clarification={}, abstain={}, l1={:?}@{:.2}, l2_present={}, l2_applied={}, l2_confidence={:?}, override_reason={:?}, merged_continuation={}, l0(path={},err={},short={},git={},cmd={})",
        assessment.confidence,
        assessment.need_clarification,
        assessment.abstain,
        merge_meta.l1_kind,
        merge_meta.l1_confidence,
        merge_meta.l2_present,
        merge_meta.l2_applied,
        merge_meta.l2_confidence,
        merge_meta.override_reason,
        merge_meta.used_merged_continuation,
        l0.has_file_path_like,
        l0.has_error_signal,
        l0.is_short,
        l0.has_git_keyword,
        l0.has_command_cargo
    )
}

async fn emit_intent_analysis_sse(
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    assessment: &IntentDecision,
    merge_meta: &crate::agent::intent_pipeline::IntentMergeMeta,
) {
    let Some(out) = out else {
        return;
    };
    let _ = sse::send_string_logged(
        out,
        sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "intent_analysis".to_string(),
                title: format_intent_analysis_title(assessment),
                detail: Some(format_intent_analysis_detail(assessment, merge_meta)),
            },
        }),
        "hierarchical::intent_analysis",
    )
    .await;
}

/// 运行分层多 Agent
pub(crate) async fn run_hierarchical_agent(
    p: &mut RunLoopParams<'_>,
) -> Result<(), RunAgentTurnError> {
    // 获取用户消息
    let in_clarification_flow = recently_waiting_execute_confirmation(p.messages);
    let task = extract_effective_user_task(p.messages, in_clarification_flow);
    if task.is_empty() {
        log::warn!(target: "crabmate", "Hierarchical mode: no user task found");
        return Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: "Hierarchical mode requires a user message".to_string(),
        });
    }

    let recent_user_messages = collect_recent_user_messages(p.messages, 4);
    let intent_ctx = IntentContext {
        recent_user_messages,
        in_clarification_flow,
        thresholds: ExecuteIntentThresholds {
            low: p.cfg.intent_execute_low_threshold,
            high: p.cfg.intent_execute_high_threshold,
        },
        l2_min_confidence: p.cfg.intent_l2_min_confidence,
        l0_routing_boost_enabled: p.cfg.intent_l0_routing_boost_enabled,
    };
    let (routing_for_l1, _, _) =
        crate::agent::intent_pipeline::prepare_intent_routing(&task, &intent_ctx);
    let l2_candidate = if p.cfg.intent_l2_enabled {
        classify_intent_l2_with_llm(
            &routing_for_l1,
            &task,
            p.cfg.as_ref(),
            p.llm_backend,
            p.client,
            p.api_key,
        )
        .await
    } else {
        None
    };
    let (assessment, merge_meta) = assess_and_route_with_l2(&task, &intent_ctx, l2_candidate);
    log::info!(
        target: "crabmate",
        "[INTENT_PIPELINE] l1_kind={:?} l1_confidence={:.2} l2_present={} l2_applied={} l2_confidence={:?} override_reason={:?} final_kind={:?} primary_intent={} confidence={:.2} abstain={} need_clarification={} action={:?}",
        merge_meta.l1_kind,
        merge_meta.l1_confidence,
        merge_meta.l2_present,
        merge_meta.l2_applied,
        merge_meta.l2_confidence,
        merge_meta.override_reason,
        assessment.kind,
        assessment.primary_intent,
        assessment.confidence,
        assessment.abstain,
        assessment.need_clarification,
        assessment.action
    );
    emit_intent_analysis_sse(p.out, &assessment, &merge_meta).await;

    match assessment.action {
        IntentAction::Execute => {}
        IntentAction::DirectReply(reply)
        | IntentAction::ClarifyThenExecute(reply)
        | IntentAction::ConfirmThenExecute(reply) => {
            p.messages
                .push(crate::types::Message::assistant_only(reply.clone()));
            if let Some(out) = p.out {
                let phase_payload =
                    sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
                        assistant_answer_phase: true,
                    });
                let _ =
                    sse::send_string_logged(out, phase_payload, "hierarchical::answer_phase").await;
                // 优先走 plain delta 正文链路，确保前端 assistant 气泡实时追加。
                let _ = sse::send_string_logged(
                    out,
                    reply.clone(),
                    "hierarchical::final_response_delta",
                )
                .await;
                // 兼容旧前端：保留 final_response timeline 事件（用于旧版本回写正文/时间线）。
                let final_tl = sse::encode_message(crate::sse::SsePayload::TimelineLog {
                    log: crate::sse::protocol::TimelineLogBody {
                        kind: "final_response".to_string(),
                        title: reply,
                        detail: None,
                    },
                });
                let _ =
                    sse::send_string_logged(out, final_tl, "hierarchical::final_response").await;
            }
            return Ok(());
        }
    }

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

/// 取当前 user 条之前的最近 `max` 条 user 正文（**新在前**），供 L0 续接合并。
fn collect_recent_user_messages(messages: &[crate::types::Message], max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in messages.iter().rev() {
        if m.role != "user" {
            continue;
        }
        if out.len() > max {
            break;
        }
        if let Some(t) = crate::types::message_content_as_str(&m.content) {
            let s = t.trim();
            if !s.is_empty() {
                out.push(s.to_string());
            }
        }
    }
    if out.is_empty() {
        return Vec::new();
    }
    // 去掉当前（最新）user，只保留前序
    out.remove(0);
    out
}

/// 从消息中提取用户任务
fn extract_user_task(messages: &[crate::types::Message]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| crate::types::message_content_as_str(&m.content))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn extract_effective_user_task(
    messages: &[crate::types::Message],
    in_clarification_flow: bool,
) -> String {
    let latest = extract_user_task(messages);
    if !in_clarification_flow {
        return latest;
    }
    let latest_norm = latest.trim().to_lowercase();
    if !is_explicit_execute_confirmation(&latest_norm) {
        return latest;
    }

    let mut seen_latest_user = false;
    for m in messages.iter().rev() {
        if m.role != "user" {
            continue;
        }
        let Some(content) = crate::types::message_content_as_str(&m.content) else {
            continue;
        };
        let t = content.trim();
        if t.is_empty() {
            continue;
        }
        if !seen_latest_user {
            seen_latest_user = true;
            continue;
        }
        let norm = t.to_lowercase();
        if !is_explicit_execute_confirmation(&norm) {
            return t.to_string();
        }
    }
    latest
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
    use super::{extract_effective_user_task, format_intent_analysis_title};
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
        let task = extract_effective_user_task(&messages, true);
        assert_eq!(task, "编写一个简单c++程序并执行");
    }

    #[test]
    fn normal_latest_user_task_kept_when_not_confirmation() {
        let messages = vec![
            Message::user_only("先看看目录".to_string()),
            Message::assistant_only("好的".to_string()),
            Message::user_only("编写一个简单c++程序并执行".to_string()),
        ];
        let task = extract_effective_user_task(&messages, false);
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
        let title = format_intent_analysis_title(&assessment);
        assert!(title.contains("kind=Execute"));
        assert!(title.contains("primary=execute.code_change"));
        assert!(title.contains("action=直接执行"));
    }
}
