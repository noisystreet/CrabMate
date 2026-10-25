//! 单轮 `run_agent_turn` 编排（从 `lib.rs` 拆出以降低单文件行数）。

use std::sync::Arc;

use tracing::Instrument;

use crate::llm as llm_mod;
use crate::{
    AgentTurnLlmOverrides, AgentTurnTransport, RunAgentTurnAttach, RunAgentTurnObs,
    RunAgentTurnParams, RunAgentTurnSession, RunAgentTurnSharedInputs,
};

fn resolved_turn_llm_backend<'a>(
    llm_backend: Option<&'a (dyn llm_mod::ChatCompletionsBackend + 'static)>,
) -> &'a (dyn llm_mod::ChatCompletionsBackend + 'static) {
    match llm_backend {
        Some(b) => b,
        None => llm_mod::default_chat_completions_backend(),
    }
}

async fn emit_mcp_servers_skipped_notice(
    skipped: &[crate::mcp::McpServerSkipInfo],
    out: Option<&tokio::sync::mpsc::Sender<String>>,
) {
    if skipped.is_empty() {
        return;
    }
    let detail = skipped
        .iter()
        .map(|s| format!("{} ({}): {}", s.name, s.id, s.error))
        .collect::<Vec<_>>()
        .join("\n");
    let title = if skipped.len() == 1 {
        format!("MCP 服务器连接失败：{}", skipped[0].name)
    } else {
        format!("MCP：{} 个服务器连接失败", skipped.len())
    };
    log::warn!(target: "crabmate", "MCP 本轮跳过服务器：\n{detail}");
    crate::turn_replay_dump::append_turn_replay_event_if_configured(
        "mcp_servers_skipped",
        title.as_str(),
        Some(detail.as_str()),
    );
    if let Some(tx) = out {
        let payload = crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "mcp_servers_skipped".to_string(),
                title,
                detail: Some(detail),
            },
        };
        let _ = crate::sse::send_string_logged(
            tx,
            crate::sse::encode_message(payload),
            "mcp_servers_skipped",
        )
        .await;
    }
}

/// 执行一轮 Agent：发请求、若遇 tool_calls 则执行工具并继续，直到模型返回最终回复。
/// `cfg` 建议使用 [`Arc`] 共享（与进程内 Web 服务状态一致），以便工具在 `spawn_blocking` 路径中复用同一份配置而不反复深拷贝。
/// 若提供 transport.out，则流式 content 会通过 out 发送（供 SSE 等使用）；`transport.no_stream` 为 true 时 API 使用 `stream: false`，
/// 有正文则通过 `out` 一次性下发整段。
/// effective_working_dir 为当前生效的工作目录（可与前端设置的工作区一致）。
/// `transport.cancel` 为 `Some` 时，各轮请求会在流式读与重试间隔中轮询其标志；置位后尽快结束并返回 `Ok`（或 `Err`：`agent::agent_turn::RunAgentTurnError` 中含取消 / 限流 / SSE 早停等，用户可见串与常量 [`crate::types::LLM_CANCELLED_ERROR`] 对齐），供协作取消等场景使用。
/// 分阶段规划下若规划轮未解析出合法 `agent_reply_plan` v1：**不再**整轮失败退出：保留规划轮助手正文并**降级**为与门控拒绝时相同的常规 `run_agent_outer_loop`（含工具）。规划轮会先丢弃 API 返回的原生 `tool_calls`，再视情况执行工具，避免网关误报 `tool_calls` 时静默无动作。
/// `transport.per_flight` 仅 Web 队列任务传入，用于 `GET /status` 的 `per_active_jobs` 镜像；运维 CLI 传 `None`。
/// 自定义 `ChatCompletionsBackend` 见 [`AgentTurnTransport::llm_backend`]。
pub async fn run_agent_turn<'a>(
    p: RunAgentTurnParams<'a>,
) -> Result<crate::agent::model_context_view::StructuralCommit, crate::agent::agent_turn::RunAgentTurnError>
{
    let RunAgentTurnParams {
        shared,
        session,
        transport,
        llm,
        attach,
        obs,
    } = p;
    let RunAgentTurnSharedInputs {
        client,
        api_key,
        cfg,
        tools,
    } = shared;
    let RunAgentTurnSession {
        messages,
        effective_working_dir,
        workspace_is_set,
    } = session;
    let RunAgentTurnAttach {
        long_term_memory,
        long_term_memory_scope_id,
        read_file_turn_cache,
        turn_allowed_tool_names,
        session_mode,
    } = attach;
    let RunAgentTurnObs {
        tracing_chat_turn,
        request_audit,
        process_handles,
        tool_job_registry,
    } = obs;
    let AgentTurnTransport {
        out,
        no_stream,
        cancel,
        per_flight,
        web_tool_ctx,
        sse_control_mirror,
        llm_backend,
        trace_sink,
    } = transport;
    let AgentTurnLlmOverrides {
        temperature_override,
        model_override,
        use_executor_model,
        executor_model_override,
        executor_api_base,
        executor_api_key,
        seed_override,
    } = llm;
    let turn_dump_scope_id = long_term_memory_scope_id.clone();
    let turn_dump_model_override = model_override.clone();
    let turn_dump_executor_model_override = executor_model_override.clone();
    let llm_backend = resolved_turn_llm_backend(llm_backend);

    // 出站 `@` 图跟本轮工具 `work_dir`，不读热更新后的全局工作区。
    let cfg_for_turn = {
        let mut snap = (**cfg).clone();
        snap.chat_workspace_root = Some(effective_working_dir.to_path_buf());
        Arc::new(snap)
    };
    let cfg = &cfg_for_turn;

    let read_file_turn_cache =
        crate::agent_turn_prep::resolve_read_file_turn_cache_for_turn(cfg, read_file_turn_cache);

    let workspace_changelist = crate::agent_turn_prep::workspace_changelist_for_turn(
        cfg.as_ref(),
        process_handles.as_ref(),
        long_term_memory_scope_id.as_deref(),
    );

    let crate::agent_turn_prep::ToolsForTurnPrepared {
        tools_for_turn,
        mcp_turn,
        mcp_skipped,
    } = crate::agent_turn_prep::prepare_tools_for_turn(
        cfg,
        tools,
        effective_working_dir,
        turn_allowed_tool_names.as_ref().map(|a| a.as_ref()),
    )
    .await;
    emit_mcp_servers_skipped_notice(&mcp_skipped, out).await;

    let request_chrome_trace = crate::request_chrome_trace::request_trace_dir_from_env()
        .map(|_| Arc::new(crate::request_chrome_trace::RequestTurnTrace::new()));
    let wall_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    record_turn_start_replay(
        messages,
        wall_ms,
        turn_dump_scope_id.as_deref(),
        tracing_chat_turn.as_ref().map(|t| t.job_id),
    );

    let mut model_context_view =
        crate::agent::model_context_view::ModelContextView::derive(messages);
    let mut loop_params = crate::agent::agent_turn::RunLoopParams {
        ctx: crate::agent::agent_turn::RunLoopCtx {
            core: crate::agent::agent_turn::RunLoopCore {
                llm_backend,
                client,
                api_key,
                cfg,
                tools_defs: tools_for_turn.as_slice(),
                effective_working_dir,
                workspace_is_set,
            },
            io: crate::agent::agent_turn::RunLoopIo {
                no_stream,
                cancel: cancel.clone(),
                control: crate::agent::agent_turn::TurnControlSink {
                    out,
                    sse_encoder: crate::sse::default_encoder(),
                    sse_control_mirror,
                },
            },
            attach: crate::agent::agent_turn::RunLoopAttach {
                web_tool_ctx,
                per_flight,
                long_term_memory,
                long_term_memory_scope_id,
                mcp_turn,
                read_file_turn_cache,
                workspace_changelist,
                turn_allowed_tool_names: turn_allowed_tool_names.clone(),
                session_mode,
            },
            obs: crate::agent::agent_turn::RunLoopObs {
                request_chrome_trace: request_chrome_trace.clone(),
                tracing_chat_turn: tracing_chat_turn.clone(),
                request_audit: request_audit.clone(),
                process_handles: Arc::clone(&process_handles),
                trace_sink,
                tool_job_registry,
            },
        },
        turn: crate::agent::agent_turn::RunLoopTurnState {
            messages_buf: model_context_view.messages_mut(),
            messages_revision: 0,
            sub_phase: crate::agent::agent_turn::AgentTurnSubPhase::Planner,
            turn_planner_hints: crate::agent::agent_turn::TurnPlannerHints::default(),
            temperature_override,
            model_override,
            use_executor_model,
            executor_model_override,
            executor_api_base,
            executor_api_key,
            seed_override,
            turn_budget: crate::agent::turn_budget::TurnBudgetCounter::new_shared(),
            context_timeline: Default::default(),
            model_context_artifacts: Vec::new(),
            provider_usage: Arc::new(std::sync::Mutex::new(None)),
        },
    };

    let res = run_agent_turn_common_with_optional_trace(&mut loop_params, wall_ms).await;
    // 中断 / 取消路径同样要 flush：本轮已产出的 timeline 标记须随部分输出一起落盘，
    // 否则下一轮上下文看不到「已产出到哪一步」。
    let interrupted_cancel = matches!(res.as_ref(), Err(e) if e.is_interrupted_cancel());
    if res.is_ok() || interrupted_cancel {
        loop_params.turn.flush_context_timeline_markers();
    }
    // 失败时向 trace_sink emit Error 事件（最小化 emit；完整 LLM/工具事件由后续 PR 接入）
    if let Err(e) = &res
        && let Some(sink) = loop_params.ctx.obs.trace_sink.as_ref()
    {
        sink.emit(crate::cm_llm::TraceEvent::Error {
            round: 0,
            kind: "turn_failed".to_string(),
            message: e.to_string(),
        })
        .await;
    }
    write_agent_turn_replay_dump(crate::turn_replay_dump::TurnReplayDumpParams {
        wall_ms,
        long_term_memory_scope_id: turn_dump_scope_id.as_deref(),
        tracing_job_id: tracing_chat_turn.as_ref().map(|t| t.job_id),
        result: &res,
        messages: loop_params.turn.messages(),
        tools: tools_for_turn.as_slice(),
        cfg: loop_params.ctx.core.cfg,
        no_stream,
        effective_working_dir,
        workspace_is_set,
        temperature_override,
        model_override: turn_dump_model_override,
        use_executor_model,
        executor_model_override: turn_dump_executor_model_override,
        seed_override,
    });
    let mut model_context_artifacts = std::mem::take(&mut loop_params.turn.model_context_artifacts);
    drop(loop_params);
    finish_turn_result(
        res,
        interrupted_cancel,
        messages,
        &model_context_view,
        &mut model_context_artifacts,
    )
}

/// `run_agent_turn` 收尾：正常完成 → 提交视图；中断 / 取消 → 仅追加写回部分输出后仍返回 `Err`。
///
/// 中断 / 取消时调用方（job 队列）会据部分输出落盘，故这里把本轮已产出的消息**仅追加**写回
/// canonical（不压缩、不归档，见 [`ModelContextView::append_current_turn_to`]），并补齐可能停在
/// 半途的 tool 响应（`assistant(tool_calls=[A,B]) + tool(A)` 是非法序列，下一轮会 400）。
/// 错误语义不变，仍返回 `Err`。
fn finish_turn_result(
    res: Result<(), crate::agent::agent_turn::RunAgentTurnError>,
    interrupted_cancel: bool,
    messages: &mut Vec<crate::types::Message>,
    model_context_view: &crate::agent::model_context_view::ModelContextView,
    model_context_artifacts: &mut Vec<crate::agent::model_context_view::ModelContextArtifact>,
) -> Result<
    crate::agent::model_context_view::StructuralCommit,
    crate::agent::agent_turn::RunAgentTurnError,
> {
    match res {
        Ok(()) => Ok(commit_successful_model_context_view(
            messages,
            model_context_view,
            model_context_artifacts,
        )),
        Err(e) if interrupted_cancel => {
            let appended = model_context_view.append_current_turn_to(messages);
            let completed = crate::cm_agent::message_pipeline::
                complete_pending_tool_responses_for_persistence(messages);
            // 中断分支**不做**结构性提交：本轮模型视图的压缩配方（artifacts）描述的是一个
            // 未被 commit 的视图，若转成 marker 写入 canonical，下一轮会把压缩区间重放到
            // 实际并不存在的位置。显式丢弃，也让「中断路径不消费 artifacts」成为显式契约。
            model_context_artifacts.clear();
            log::debug!(
                target: "crabmate",
                "中断回合写回部分输出：appended={appended} completed_tool_responses={completed}"
            );
            Err(e)
        }
        Err(e) => Err(e),
    }
}

fn record_turn_start_replay(
    messages: &[crate::types::Message],
    wall_ms: u64,
    scope_id: Option<&str>,
    job_id: Option<u64>,
) {
    crate::turn_replay_dump::set_turn_replay_event_context(wall_ms, scope_id, job_id);
    crate::turn_replay_dump::append_latest_user_input_event_if_configured(messages);
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "turn_started",
        "run_agent_turn",
        Some(&serde_json::json!({
            "text": format!("wall_start_ms={wall_ms}"),
            "phase": "turn"
        })),
    );
}

fn commit_successful_model_context_view(
    canonical: &mut Vec<crate::types::Message>,
    model_context_view: &crate::agent::model_context_view::ModelContextView,
    artifacts: &mut Vec<crate::agent::model_context_view::ModelContextArtifact>,
) -> crate::agent::model_context_view::StructuralCommit {
    for artifact in artifacts.iter_mut() {
        artifact.canonical_message_count_before_turn =
            model_context_view.canonical_message_count_before_turn();
        artifact.bind_canonical_ranges(canonical);
    }
    for marker in artifacts.drain(..).filter_map(|artifact| artifact.into_marker()) {
        canonical.push(marker);
    }
    model_context_view.commit_current_turn_to(canonical)
}

async fn run_agent_turn_common_with_optional_trace(
    loop_params: &mut crate::agent::agent_turn::RunLoopParams<'_>,
    wall_ms: u64,
) -> Result<(), crate::agent::agent_turn::RunAgentTurnError> {
    let trace_span = loop_params
        .ctx
        .obs
        .tracing_chat_turn
        .as_ref()
        .map(|t| t.span.clone());
    let request_chrome_trace = loop_params.ctx.obs.request_chrome_trace.clone();
    let run_common = crate::agent::agent_turn::run_agent_turn_common(loop_params);
    match (trace_span, request_chrome_trace) {
        (Some(span), Some(t)) => {
            crate::request_chrome_trace::with_turn_trace(t, wall_ms, run_common.instrument(span))
                .await
        }
        (Some(span), None) => run_common.instrument(span).await,
        (None, Some(t)) => {
            crate::request_chrome_trace::with_turn_trace(t, wall_ms, run_common).await
        }
        (None, None) => run_common.await,
    }
}

fn write_agent_turn_replay_dump(params: crate::turn_replay_dump::TurnReplayDumpParams<'_>) {
    let wall_ms = params.wall_ms;
    let ok = params.result.is_ok();
    crate::turn_replay_dump::write_turn_replay_dump_if_configured(params);
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "turn_finished",
        "run_agent_turn",
        Some(&serde_json::json!({
            "text": format!("wall_start_ms={wall_ms}, ok={ok}"),
            "phase": "turn"
        })),
    );
    crate::turn_replay_dump::clear_turn_replay_event_context();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_turn::errors::TurnAbortReason;
    use crate::agent::agent_turn::{AgentTurnSubPhase, RunAgentTurnError};
    use crate::agent::model_context_view::{ModelContextArtifact, ModelContextView};
    use crate::types::Message;

    #[test]
    fn finish_turn_result_on_interrupt_appends_partial_output_and_returns_err() {
        let canonical = vec![
            Message::system_only("system"),
            Message::user_only("current user"),
        ];
        let mut view = ModelContextView::derive(&canonical);
        view.messages_mut()
            .push(Message::assistant_only("partial output"));

        let mut messages = canonical.clone();
        let mut artifacts = vec![ModelContextArtifact::capture(
            1,
            canonical.len(),
            &canonical,
            Default::default(),
            None,
        )];

        let result = finish_turn_result(
            Err(RunAgentTurnError::TurnAborted {
                phase: AgentTurnSubPhase::Planner,
                reason: TurnAbortReason::UserCancelled,
            }),
            true,
            &mut messages,
            &view,
            &mut artifacts,
        );

        assert!(result.is_err());
        assert_eq!(messages.len(), canonical.len() + 1);
        assert_eq!(
            crate::types::message_content_as_str(&messages.last().unwrap().content),
            Some("partial output")
        );
        // 中断分支不做结构性提交，显式丢弃 artifacts。
        assert!(artifacts.is_empty());
    }

    #[test]
    fn finish_turn_result_on_non_interrupt_err_leaves_messages_untouched() {
        let mut messages = vec![Message::user_only("current user")];
        let before = messages.clone();
        let view = ModelContextView::derive(&messages);
        let mut artifacts = Vec::new();

        let result = finish_turn_result(
            Err(RunAgentTurnError::Other {
                phase: AgentTurnSubPhase::Planner,
                message: "boom".to_string(),
            }),
            false,
            &mut messages,
            &view,
            &mut artifacts,
        );

        assert!(result.is_err());
        assert_eq!(messages, before);
    }
}
