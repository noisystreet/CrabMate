//! 队列 worker：在独立 task 中执行 `run_agent_turn`（流式与 JSON 模式）。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use log::{debug, error, info, warn};
use tokio::sync::{mpsc, oneshot};

use crate::AppState;
use crate::agent::agent_turn::AgentTurnJobOutcomeKind;
use crate::agent_role_turn::{filter_tools_for_agent_role, turn_allow_for_web_or_cli_job};
use crate::types::{LlmSeedOverride, Message};
use crate::web::audit::WebRequestAudit;

use super::stream_finish::{
    StreamJobOutcomeCtx, emit_stream_cancelled_terminal, emit_stream_ended_once,
    post_turn_web_prepare_and_save, stream_job_outcome_after_agent_turn,
};
use super::{
    ChatJsonJobFailure, PerTurnFlight, QueuedChatJob, WebApprovalSession, WebChatLlmOverride,
    WebChatQueueDeps, WebExecutionModeOverride, resolve_executor_llm_for_job,
    resolve_web_llm_for_job,
};

pub(super) enum JobOutcome {
    Stream {
        ok: bool,
        cancelled: bool,
        err: Option<String>,
    },
    Json {
        ok: bool,
        cancelled: bool,
        err: Option<String>,
    },
}

/// `run_stream_queued_job` 入参（与 `QueuedChatJob::Stream` 字段一致；单结构体避免超长形参列表）。
struct StreamQueuedJobParams {
    job_id: u64,
    queue_deps: Arc<WebChatQueueDeps>,
    app: Arc<AppState>,
    conversation_id: String,
    messages: Vec<Message>,
    expected_revision: Option<u64>,
    request_agent_role: Option<String>,
    persisted_active_agent_role: Option<String>,
    work_dir: PathBuf,
    workspace_is_set: bool,
    temperature_override: Option<f32>,
    seed_override: LlmSeedOverride,
    llm_override: Option<WebChatLlmOverride>,
    executor_llm_override: Option<WebChatLlmOverride>,
    execution_mode_override: Option<WebExecutionModeOverride>,
    stream_event_tx: mpsc::Sender<(u64, String)>,
    web_approval_session: Option<WebApprovalSession>,
    request_audit: WebRequestAudit,
}

async fn run_stream_queued_job(p: StreamQueuedJobParams) -> JobOutcome {
    let StreamQueuedJobParams {
        job_id,
        queue_deps,
        app,
        conversation_id,
        mut messages,
        expected_revision,
        request_agent_role,
        persisted_active_agent_role,
        work_dir,
        workspace_is_set,
        temperature_override,
        seed_override,
        llm_override,
        executor_llm_override,
        execution_mode_override,
        stream_event_tx,
        web_approval_session,
        request_audit,
    } = p;
    queue_deps.sse_stream_hub.register_job(job_id);
    let hub_bridge = queue_deps.sse_stream_hub.clone();
    let bridge_job = job_id;
    let http_tx = stream_event_tx;
    let (sse_tx, mut sse_rx) = mpsc::channel::<String>(1024);
    tokio::spawn(async move {
        while let Some(line) = sse_rx.recv().await {
            if let Some(pair) = hub_bridge.publish(bridge_job, line) {
                let _ = http_tx.send(pair).await;
            }
        }
    });
    info!(
        target: "crabmate",
        "chat stream 任务开始执行 job_id={}",
        job_id
    );
    debug!(
        target: "crabmate",
        "chat stream 执行上下文 job_id={} message_count={} last_user_preview={}",
        job_id,
        messages.len(),
        crate::redact::last_user_message_preview_for_log(&messages)
    );
    let flight = Arc::new(PerTurnFlight::default());
    let _per_guard = queue_deps
        .chat_queue
        .begin_per_flight_job(job_id, flight.clone());
    let caps_line = crate::sse::encode_message(crate::sse::SsePayload::SseCapabilities {
        caps: crate::sse::SseCapabilitiesBody {
            supported_sse_v: crate::sse::protocol::SSE_PROTOCOL_VERSION,
            resume_ring_cap: crate::sse::protocol::SSE_RESUME_RING_CAP,
            job_id,
        },
    });
    let _ = crate::sse::send_string_logged(
        &sse_tx,
        caps_line,
        "chat_job_queue::stream sse_capabilities",
    )
    .await;
    let (web_tool_ctx, approval_session_id) = if let Some(session) = web_approval_session {
        (
            Some(crate::tool_registry::WebToolRuntime {
                out_tx: sse_tx.clone(),
                approval_rx_shared: Arc::new(tokio::sync::Mutex::new(session.approval_rx)),
                approval_request_guard: Arc::new(tokio::sync::Mutex::new(())),
                persistent_allowlist_shared: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            }),
            Some(session.session_id),
        )
    } else {
        (None, None)
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_watcher = {
        let tx_for_watch = sse_tx.clone();
        let cancel_for_watch = Arc::clone(&cancel);
        let job_id_watch = job_id;
        tokio::spawn(async move {
            tx_for_watch.closed().await;
            cancel_for_watch.store(true, Ordering::SeqCst);
            info!(
                target: "crabmate",
                "chat stream SSE 接收端关闭，已请求取消 job_id={}",
                job_id_watch
            );
        })
    };
    let cfg_snap = {
        let g = queue_deps.cfg.read().await;
        std::sync::Arc::new(g.clone())
    };
    let (cfg_turn, api_key_turn) = resolve_web_llm_for_job(
        queue_deps.as_ref(),
        cfg_snap.clone(),
        llm_override.as_ref(),
        execution_mode_override,
    );
    let turn_allow = turn_allow_for_web_or_cli_job(
        &cfg_turn,
        persisted_active_agent_role.as_deref(),
        request_agent_role.as_deref(),
    );
    let tools_for_job =
        filter_tools_for_agent_role(&queue_deps.tools, turn_allow.as_ref().map(|a| a.as_ref()));
    let executor_override = resolve_executor_llm_for_job(
        &queue_deps,
        Arc::clone(&cfg_turn),
        executor_llm_override.as_ref(),
    );
    let (executor_api_base, executor_api_key, executor_model_override) = match executor_override {
        Some((executor_cfg, executor_key)) => {
            let base = if executor_cfg.api_base != cfg_turn.api_base {
                Some(executor_cfg.api_base.clone())
            } else {
                None
            };
            let model = if executor_cfg.model != cfg_turn.model {
                Some(executor_cfg.model.clone())
            } else {
                None
            };
            (base, Some(executor_key), model)
        }
        None => (None, None, None),
    };
    let r = crate::run_agent_turn(crate::RunAgentTurnParams::web_chat_stream(
        crate::WebChatStreamBuildArgs {
            client: &queue_deps.client,
            api_key: api_key_turn.as_str(),
            cfg: &cfg_turn,
            tools: tools_for_job.as_slice(),
            messages: &mut messages,
            effective_working_dir: &work_dir,
            workspace_is_set,
            cancel: Arc::clone(&cancel),
            per_flight: flight,
            web_tool_ctx: web_tool_ctx.as_ref(),
            temperature_override,
            model_override: None, // planner 阶段不使用前端传来的 executor model
            use_executor_model: false, // first iteration is always planner round
            executor_model_override, // 前端传来的 executor_llm.model
            executor_api_base,
            executor_api_key,
            seed_override,
            long_term_memory: queue_deps.long_term_memory.clone(),
            job_id,
            conversation_id: conversation_id.as_str(),
            out: &sse_tx,
            turn_allowed_tool_names: turn_allow,
            request_audit: std::sync::Arc::new(request_audit),
            process_handles: Arc::clone(&app.process_handles),
        },
    ))
    .await;
    cancel_watcher.abort();
    if let Some(session_id) = approval_session_id.as_deref() {
        app.approval_sessions.write().await.remove(session_id);
    }
    let cancelled_by_signal = cancel.load(Ordering::SeqCst);
    let mut stream_ended_sent = false;
    let (ok, cancelled, err, stream_end_reason) =
        stream_job_outcome_after_agent_turn(StreamJobOutcomeCtx {
            r,
            cancelled_by_signal,
            queue_deps: queue_deps.as_ref(),
            sse_tx: &sse_tx,
            job_id,
            messages: &mut messages,
            cfg_snap: &cfg_snap,
            app: app.as_ref(),
            conversation_id: conversation_id.as_str(),
            expected_revision,
            request_agent_role: request_agent_role.as_deref(),
            persisted_active_agent_role: persisted_active_agent_role.as_deref(),
            stream_ended_sent: &mut stream_ended_sent,
        })
        .await;
    if cancelled {
        emit_stream_cancelled_terminal(&sse_tx, job_id).await;
    }
    if !stream_ended_sent {
        emit_stream_ended_once(
            &sse_tx,
            job_id,
            stream_end_reason,
            &mut stream_ended_sent,
            "chat_job_queue::stream stream_ended",
        )
        .await;
    }
    drop(sse_tx);
    queue_deps.sse_stream_hub.remove_job(job_id);
    JobOutcome::Stream { ok, cancelled, err }
}

/// `run_json_queued_job` 入参（与 `QueuedChatJob::Json` 字段一致）。
struct JsonQueuedJobParams {
    job_id: u64,
    queue_deps: Arc<WebChatQueueDeps>,
    app: Arc<AppState>,
    conversation_id: String,
    messages: Vec<Message>,
    expected_revision: Option<u64>,
    request_agent_role: Option<String>,
    persisted_active_agent_role: Option<String>,
    work_dir: PathBuf,
    workspace_is_set: bool,
    temperature_override: Option<f32>,
    seed_override: LlmSeedOverride,
    llm_override: Option<WebChatLlmOverride>,
    executor_llm_override: Option<WebChatLlmOverride>,
    execution_mode_override: Option<WebExecutionModeOverride>,
    reply_tx: oneshot::Sender<Result<Vec<Message>, ChatJsonJobFailure>>,
    request_audit: WebRequestAudit,
}

async fn run_json_queued_job(p: JsonQueuedJobParams) -> JobOutcome {
    let JsonQueuedJobParams {
        job_id,
        queue_deps,
        app,
        conversation_id,
        mut messages,
        expected_revision,
        request_agent_role,
        persisted_active_agent_role,
        work_dir,
        workspace_is_set,
        temperature_override,
        seed_override,
        llm_override,
        executor_llm_override,
        execution_mode_override,
        reply_tx,
        request_audit,
    } = p;
    info!(
        target: "crabmate",
        "chat json 任务开始执行 job_id={}",
        job_id
    );
    debug!(
        target: "crabmate",
        "chat json 执行上下文 job_id={} message_count={} last_user_preview={}",
        job_id,
        messages.len(),
        crate::redact::last_user_message_preview_for_log(&messages)
    );
    let flight = Arc::new(PerTurnFlight::default());
    let _per_guard = queue_deps
        .chat_queue
        .begin_per_flight_job(job_id, flight.clone());
    let cfg_snap = {
        let g = queue_deps.cfg.read().await;
        std::sync::Arc::new(g.clone())
    };
    let (cfg_turn, api_key_turn) = resolve_web_llm_for_job(
        queue_deps.as_ref(),
        cfg_snap.clone(),
        llm_override.as_ref(),
        execution_mode_override,
    );
    let turn_allow = turn_allow_for_web_or_cli_job(
        &cfg_turn,
        persisted_active_agent_role.as_deref(),
        request_agent_role.as_deref(),
    );
    let tools_for_job =
        filter_tools_for_agent_role(&queue_deps.tools, turn_allow.as_ref().map(|a| a.as_ref()));
    let executor_override = resolve_executor_llm_for_job(
        &queue_deps,
        Arc::clone(&cfg_turn),
        executor_llm_override.as_ref(),
    );
    let (executor_api_base, executor_api_key, executor_model_override) = match executor_override {
        Some((executor_cfg, executor_key)) => {
            let base = if executor_cfg.api_base != cfg_turn.api_base {
                Some(executor_cfg.api_base.clone())
            } else {
                None
            };
            let model = if executor_cfg.model != cfg_turn.model {
                Some(executor_cfg.model.clone())
            } else {
                None
            };
            (base, Some(executor_key), model)
        }
        None => (None, None, None),
    };
    let r = crate::run_agent_turn(crate::RunAgentTurnParams::web_chat_json(
        crate::WebChatJsonBuildArgs {
            client: &queue_deps.client,
            api_key: api_key_turn.as_str(),
            cfg: &cfg_turn,
            tools: tools_for_job.as_slice(),
            messages: &mut messages,
            effective_working_dir: &work_dir,
            workspace_is_set,
            per_flight: flight,
            temperature_override,
            model_override: None,
            use_executor_model: false,
            executor_model_override,
            executor_api_base,
            executor_api_key,
            seed_override,
            long_term_memory: queue_deps.long_term_memory.clone(),
            job_id,
            conversation_id: conversation_id.as_str(),
            turn_allowed_tool_names: turn_allow,
            request_audit: std::sync::Arc::new(request_audit),
            process_handles: Arc::clone(&app.process_handles),
        },
    ))
    .await;
    let (ok, cancelled, err) = match r {
        Ok(()) => {
            match post_turn_web_prepare_and_save(
                app.as_ref(),
                &cfg_snap,
                &conversation_id,
                &mut messages,
                expected_revision,
                request_agent_role.as_deref(),
                persisted_active_agent_role.as_deref(),
            )
            .await
            {
                crate::SaveConversationOutcome::Saved => {
                    if reply_tx.send(Ok(messages)).is_err() {
                        debug!(
                            target: "crabmate::sse_mpsc",
                            "chat json oneshot reply failed (Ok): job_id={} receiver dropped",
                            job_id
                        );
                    }
                    (true, false, None)
                }
                crate::SaveConversationOutcome::Conflict => {
                    if reply_tx
                        .send(Err(ChatJsonJobFailure::ConversationConflict))
                        .is_err()
                    {
                        debug!(
                            target: "crabmate::sse_mpsc",
                            "chat json oneshot reply failed (CONVERSATION_CONFLICT): job_id={} receiver dropped",
                            job_id
                        );
                    }
                    (false, false, Some("conversation_conflict".to_string()))
                }
            }
        }
        Err(e) => {
            let jq_outcome = e.job_queue_json_outcome_kind();
            let cancelled = matches!(jq_outcome, AgentTurnJobOutcomeKind::UserCancelled);
            match jq_outcome {
                AgentTurnJobOutcomeKind::UserCancelled => {
                    info!(
                        target: "crabmate",
                        "chat json 任务已取消 job_id={} err_kind=cancelled {}",
                        job_id,
                        e.diag_log_kv(),
                    );
                }
                AgentTurnJobOutcomeKind::StagedPlanInvalidLegacy => {
                    warn!(
                        target: "crabmate",
                        "chat json 任务结束（分阶段规划解析失败） job_id={} err_kind=staged_plan_invalid {}",
                        job_id,
                        e.diag_log_kv(),
                    );
                }
                AgentTurnJobOutcomeKind::FailureEmitSseError => {
                    error!(
                        target: "crabmate",
                        "chat json 任务失败 job_id={} err_kind=agent_turn {}",
                        job_id,
                        e.diag_log_kv(),
                    );
                }
            }
            let prev = e.short_detail_for_job_log();
            if reply_tx.send(Err(ChatJsonJobFailure::Agent(e))).is_err() {
                debug!(
                    target: "crabmate::sse_mpsc",
                    "chat json oneshot reply failed (Err): job_id={} receiver dropped",
                    job_id
                );
            }
            (false, cancelled, prev)
        }
    };
    JobOutcome::Json { ok, cancelled, err }
}

pub(super) async fn run_queued_job(job: QueuedChatJob) -> JobOutcome {
    match job {
        QueuedChatJob::Stream {
            job_id,
            queue_deps,
            app,
            conversation_id,
            messages,
            expected_revision,
            request_agent_role,
            persisted_active_agent_role,
            work_dir,
            workspace_is_set,
            temperature_override,
            seed_override,
            llm_override,
            executor_llm_override,
            execution_mode_override,
            stream_event_tx,
            web_approval_session,
            request_audit,
        } => {
            run_stream_queued_job(StreamQueuedJobParams {
                job_id,
                queue_deps,
                app,
                conversation_id,
                messages,
                expected_revision,
                request_agent_role,
                persisted_active_agent_role,
                work_dir,
                workspace_is_set,
                temperature_override,
                seed_override,
                llm_override,
                executor_llm_override,
                execution_mode_override,
                stream_event_tx,
                web_approval_session,
                request_audit,
            })
            .await
        }
        QueuedChatJob::Json {
            job_id,
            queue_deps,
            app,
            conversation_id,
            messages,
            expected_revision,
            request_agent_role,
            persisted_active_agent_role,
            work_dir,
            workspace_is_set,
            temperature_override,
            seed_override,
            llm_override,
            executor_llm_override,
            execution_mode_override,
            reply_tx,
            request_audit,
        } => {
            run_json_queued_job(JsonQueuedJobParams {
                job_id,
                queue_deps,
                app,
                conversation_id,
                messages,
                expected_revision,
                request_agent_role,
                persisted_active_agent_role,
                work_dir,
                workspace_is_set,
                temperature_override,
                seed_override,
                llm_override,
                executor_llm_override,
                execution_mode_override,
                reply_tx,
                request_audit,
            })
            .await
        }
    }
}
