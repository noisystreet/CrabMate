//! Web `POST /chat` JSON 队列任务执行体（从 `worker/mod.rs` 拆出以降低单文件行数）。

use std::sync::Arc;

use log::{debug, error, info};
use tokio::sync::oneshot;

use crate::agent::agent_turn::AgentTurnJobOutcomeKind;
use crate::agent_role_turn::{filter_tools_for_agent_role, turn_allow_for_web_or_cli_job};
use crate::types::Message;

use super::super::stream_finish::{PostTurnWebPrepareParams, post_turn_web_prepare_and_save};
use super::super::{
    ChatJsonJobFailure, PerTurnFlight, WebChatJobEnvelope, resolve_executor_llm_for_job,
    resolve_web_llm_for_job,
};
use super::JobOutcome;

/// `run_json_queued_job` 入参（[`WebChatJobEnvelope`] + JSON oneshot）。
pub(super) struct JsonQueuedJobParams {
    pub(super) envelope: WebChatJobEnvelope,
    pub(super) reply_tx: oneshot::Sender<Result<Vec<Message>, ChatJsonJobFailure>>,
}

pub(super) async fn run_json_queued_job(p: JsonQueuedJobParams) -> JobOutcome {
    let JsonQueuedJobParams { envelope, reply_tx } = p;
    let WebChatJobEnvelope {
        job_id,
        queue_deps,
        app,
        conversation_id,
        mut messages,
        expected_revision,
        request_agent_role,
        persisted_active_agent_role,
        request_session_mode,
        persisted_active_session_mode,
        work_dir,
        workspace_is_set,
        temperature_override,
        seed_override,
        llm_override,
        executor_llm_override,
        readonly_tool_ttl_cache_secs,
        request_audit,
        client_sse_protocol: _,
        request_id,
        github_token: _,
    } = envelope;
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
    let session_mode = resolve_job_session_mode(
        cfg_snap.as_ref(),
        request_session_mode.as_deref(),
        persisted_active_session_mode.as_deref(),
        request_agent_role.as_deref(),
        persisted_active_agent_role.as_deref(),
    );
    let (mut cfg_turn, api_key_turn) =
        resolve_web_llm_for_job(queue_deps.as_ref(), cfg_snap.clone(), llm_override.as_ref());
    if let Some(secs) = readonly_tool_ttl_cache_secs {
        let mut c = (*cfg_turn).clone();
        c.chat_queues_cache.readonly_tool_ttl_cache_secs = secs;
        cfg_turn = Arc::new(c);
    }
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
    let (executor_api_base, executor_api_key, executor_model_override) =
        executor_llm_triple(executor_override.as_ref(), &cfg_turn);
    let client_model_override = llm_override.as_ref().and_then(|o| o.model.clone());
    let r = queue_deps
        .turn_runner
        .run(crate::RunAgentTurnParams::web_chat_json(
            crate::WebChatJsonBuildArgs {
                shared: crate::RunAgentTurnSharedInputs {
                    client: &queue_deps.client,
                    api_key: api_key_turn.as_str(),
                    cfg: &cfg_turn,
                    tools: tools_for_job.as_slice(),
                },
                messages: &mut messages,
                effective_working_dir: &work_dir,
                workspace_is_set,
                per_flight: flight,
                temperature_override,
                model_override: client_model_override,
                use_executor_model: false,
                executor_model_override,
                executor_api_base,
                executor_api_key,
                seed_override,
                long_term_memory: queue_deps.long_term_memory.clone(),
                job_id,
                conversation_id: conversation_id.as_str(),
                request_id,
                turn_allowed_tool_names: turn_allow,
                session_mode,
                request_audit: std::sync::Arc::new(request_audit),
                process_handles: Arc::clone(&app.process_handles),
                tool_job_registry: Some(std::sync::Arc::clone(&app.tool_job_registry)),
            },
        ))
        .await;
    let (ok, cancelled, err) = finish_json_queued_job_after_turn(
        r,
        JsonTurnFinishParams {
            reply_tx,
            job_id,
            app: &app,
            queue_deps: queue_deps.as_ref(),
            cfg_snap: &cfg_snap,
            conversation_id: &conversation_id,
            messages: &mut messages,
            expected_revision,
            request_agent_role: request_agent_role.as_deref(),
            persisted_active_agent_role: persisted_active_agent_role.as_deref(),
            request_session_mode: request_session_mode.as_deref(),
            persisted_active_session_mode: persisted_active_session_mode.as_deref(),
        },
    )
    .await;
    JobOutcome::Json { ok, cancelled, err }
}

fn resolve_job_session_mode(
    cfg: &crate::config::AgentConfig,
    request: Option<&str>,
    persisted: Option<&str>,
    request_agent_role: Option<&str>,
    persisted_agent_role: Option<&str>,
) -> crate::types::SessionMode {
    let role_default = crate::session_mode_turn::role_default_session_mode_for_turn(
        cfg,
        persisted_agent_role,
        request_agent_role,
    );
    match crate::session_mode_turn::resolve_session_mode_for_turn(
        request,
        persisted,
        role_default,
        cfg.roles_prompts.default_session_mode,
    ) {
        Ok(m) => m,
        Err(e) => {
            log::warn!(target: "crabmate", "session_mode resolve failed: {e}; using act");
            crate::types::SessionMode::Act
        }
    }
}

fn executor_llm_triple(
    executor_override: Option<&(Arc<crate::config::AgentConfig>, String)>,
    cfg_turn: &crate::config::AgentConfig,
) -> (Option<String>, Option<String>, Option<String>) {
    match executor_override {
        Some((executor_cfg, executor_key)) => {
            let base = if executor_cfg.llm.api_base != cfg_turn.llm.api_base {
                Some(executor_cfg.llm.api_base.clone())
            } else {
                None
            };
            let model = if executor_cfg.llm.model != cfg_turn.llm.model {
                Some(executor_cfg.llm.model.clone())
            } else {
                None
            };
            (base, Some(executor_key.clone()), model)
        }
        None => (None, None, None),
    }
}

struct JsonTurnFinishParams<'a> {
    reply_tx: oneshot::Sender<Result<Vec<Message>, ChatJsonJobFailure>>,
    job_id: u64,
    app: &'a crate::web::WebChatJobAppFacet,
    queue_deps: &'a super::super::WebChatQueueDeps,
    cfg_snap: &'a Arc<crate::config::AgentConfig>,
    conversation_id: &'a str,
    messages: &'a mut Vec<Message>,
    expected_revision: Option<u64>,
    request_agent_role: Option<&'a str>,
    persisted_active_agent_role: Option<&'a str>,
    request_session_mode: Option<&'a str>,
    persisted_active_session_mode: Option<&'a str>,
}

async fn finish_json_queued_job_after_turn(
    r: Result<(), crate::agent::agent_turn::RunAgentTurnError>,
    p: JsonTurnFinishParams<'_>,
) -> (bool, bool, Option<String>) {
    match r {
        Ok(()) => match post_turn_web_prepare_and_save(PostTurnWebPrepareParams {
            app: p.app,
            queue_deps: p.queue_deps,
            cfg_snap: p.cfg_snap,
            conversation_id: p.conversation_id,
            messages: p.messages,
            expected_revision: p.expected_revision,
            request_agent_role: p.request_agent_role,
            persisted_active_agent_role: p.persisted_active_agent_role,
            request_session_mode: p.request_session_mode,
            persisted_active_session_mode: p.persisted_active_session_mode,
        })
        .await
        {
            crate::SaveConversationOutcome::Saved => {
                if p.reply_tx.send(Ok(std::mem::take(p.messages))).is_err() {
                    debug!(
                        target: "crabmate::sse_mpsc",
                        "chat json oneshot reply failed (Ok): job_id={} receiver dropped",
                        p.job_id
                    );
                }
                (true, false, None)
            }
            crate::SaveConversationOutcome::Conflict => {
                if p.reply_tx
                    .send(Err(ChatJsonJobFailure::ConversationConflict))
                    .is_err()
                {
                    debug!(
                        target: "crabmate::sse_mpsc",
                        "chat json oneshot reply failed (CONVERSATION_CONFLICT): job_id={} receiver dropped",
                        p.job_id
                    );
                }
                (false, false, Some("conversation_conflict".to_string()))
            }
        },
        Err(e) => {
            let jq_outcome = e.job_queue_json_outcome_kind();
            let cancelled = matches!(jq_outcome, AgentTurnJobOutcomeKind::UserCancelled);
            match jq_outcome {
                AgentTurnJobOutcomeKind::UserCancelled => {
                    info!(
                        target: "crabmate",
                        "chat json 任务已取消 job_id={} err_kind=cancelled {}",
                        p.job_id,
                        e.diag_log_kv(),
                    );
                }
                AgentTurnJobOutcomeKind::FailureEmitSseError => {
                    error!(
                        target: "crabmate",
                        "chat json 任务失败 job_id={} err_kind=agent_turn {}",
                        p.job_id,
                        e.diag_log_kv(),
                    );
                }
            }
            let prev = e.short_detail_for_job_log();
            if p.reply_tx.send(Err(ChatJsonJobFailure::Agent(e))).is_err() {
                debug!(
                    target: "crabmate::sse_mpsc",
                    "chat json oneshot reply failed (Err): job_id={} receiver dropped",
                    p.job_id
                );
            }
            (false, cancelled, prev)
        }
    }
}
