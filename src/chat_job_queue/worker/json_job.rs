//! Web `POST /chat` JSON 队列任务执行体（从 `worker/mod.rs` 拆出以降低单文件行数）。

use std::sync::Arc;

use log::{debug, error, info, warn};
use tokio::sync::oneshot;

use crate::agent::agent_turn::AgentTurnJobOutcomeKind;
use crate::agent_role_turn::{filter_tools_for_agent_role, turn_allow_for_web_or_cli_job};
use crate::types::Message;

use super::super::stream_finish::post_turn_web_prepare_and_save;
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
        work_dir,
        workspace_is_set,
        temperature_override,
        seed_override,
        llm_override,
        executor_llm_override,
        execution_mode_override,
        readonly_tool_ttl_cache_secs,
        request_audit,
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
    let (mut cfg_turn, api_key_turn) = resolve_web_llm_for_job(
        queue_deps.as_ref(),
        cfg_snap.clone(),
        llm_override.as_ref(),
        execution_mode_override,
    );
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
    let (executor_api_base, executor_api_key, executor_model_override) = match executor_override {
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
            (base, Some(executor_key), model)
        }
        None => (None, None, None),
    };
    let r = crate::run_agent_turn(crate::RunAgentTurnParams::web_chat_json(
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
            process_handles: Arc::clone(&app.aux.process_handles),
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
