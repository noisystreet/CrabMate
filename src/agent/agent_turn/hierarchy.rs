//! 分层多 Agent 执行入口
//!
//! 当 `planner_executor_mode = Hierarchical` 时使用此模块执行任务分解和子目标执行。

use crate::agent::hierarchy::{self, HierarchyRunnerParams, HierarchyRunnerResult};
use crate::sse;

use super::errors::RunAgentTurnError;
use super::params::RunLoopParams;
use crate::agent::agent_turn::errors::AgentTurnSubPhase;

/// 运行分层多 Agent
pub(crate) async fn run_hierarchical_agent(
    p: &mut RunLoopParams<'_>,
) -> Result<(), RunAgentTurnError> {
    // 获取用户消息
    let task = extract_user_task(p.messages);
    if task.is_empty() {
        log::warn!(target: "crabmate", "Hierarchical mode: no user task found");
        return Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: "Hierarchical mode requires a user message".to_string(),
        });
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
    };

    // 运行分层 Agent
    let result = hierarchy::runner::run_hierarchical(params)
        .await
        .map_err(|e| {
            log::error!(target: "crabmate", "Hierarchical agent failed: {}", e);
            RunAgentTurnError::Other {
                phase: AgentTurnSubPhase::Planner,
                message: e.to_string(),
            }
        })?;

    // 处理执行结果
    handle_execution_result(p, result).await?;

    Ok(())
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

/// 处理分层执行结果
async fn handle_execution_result(
    p: &mut RunLoopParams<'_>,
    result: HierarchyRunnerResult,
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

    // 汇总子目标结果生成最终回复
    let final_response = aggregate_results(&execution_result.results);

    // 添加助手回复到消息
    p.messages.push(crate::types::Message::assistant_only(
        final_response.clone(),
    ));

    // 发送终答阶段信号 + 最终回答文本到 SSE
    if let Some(out) = p.out {
        // 先发送 assistant_answer_phase 使前端进入 answer 阶段
        let phase_payload = sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
            assistant_answer_phase: true,
        });
        let _ = sse::send_string_logged(out, phase_payload, "hierarchical::answer_phase").await;
        // 用 TimelineLog 发送最终回答，与 Manager 规划方式一致
        let final_tl = sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "final_response".to_string(),
                title: final_response,
                detail: None,
            },
        });
        let _ = sse::send_string_logged(out, final_tl, "hierarchical::final_response").await;
    }

    Ok(())
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
