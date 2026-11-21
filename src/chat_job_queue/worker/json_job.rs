//! Web `POST /chat` JSON 队列任务执行体（从 `worker/mod.rs` 拆出以降低单文件行数）。

use std::sync::Arc;

use log::{debug, error, info};
use tokio::sync::oneshot;

use crate::agent::agent_turn::AgentTurnJobOutcomeKind;
use crate::types::Message;

use super::super::stream_finish::{PostTurnWebPrepareParams, post_turn_web_prepare_and_save};
use super::super::{ChatJsonJobFailure, WebChatJobEnvelope};
use super::prelude::{PreparedTurnParams, prepare_queued_turn};
use super::JobOutcome;

/// `run_json_queued_job` 入参（[`WebChatJobEnvelope`] + JSON oneshot）。
pub(super) struct JsonQueuedJobParams {
    pub(super) envelope: WebChatJobEnvelope,
    pub(super) reply_tx: oneshot::Sender<Result<Vec<Message>, ChatJsonJobFailure>>,
}

pub(super) async fn run_json_queued_job(p: JsonQueuedJobParams) -> JobOutcome {
    let JsonQueuedJobParams { envelope, reply_tx } = p;
    let job_id = envelope.job_id;
    info!(
        target: "crabmate",
        "chat json 任务开始执行 job_id={}",
        job_id
    );
    debug!(
        target: "crabmate",
        "chat json 执行上下文 job_id={} message_count={} last_user_preview={}",
        job_id,
        envelope.messages.len(),
        crate::redact::last_user_message_preview_for_log(&envelope.messages)
    );

    // 前置解析（配置 / 角色 / 工具 / 权限 / executor / per-flight）与 SSE 路径共用同一份。
    let prepared = prepare_queued_turn(PreparedTurnParams {
        envelope: &envelope,
        queue_deps: envelope.queue_deps.as_ref(),
    })
    .await;

    let WebChatJobEnvelope {
        queue_deps,
        app,
        conversation_id,
        mut messages,
        expected_revision,
        request_agent_role,
        request_session_mode,
        request_permission_level,
        work_dir,
        workspace_is_set,
        temperature_override,
        seed_override,
        request_audit,
        request_id,
        ..
    } = envelope;

    let r = queue_deps
        .turn_runner
        .run(crate::TurnRequest::web_chat_json(
            crate::WebChatJsonBuildArgs {
                shared: crate::RunAgentTurnSharedInputs {
                    client: &queue_deps.client,
                    api_key: prepared.api_key_turn.as_str(),
                    cfg: &prepared.cfg_turn,
                    tools: prepared.tools_for_job.as_slice(),
                },
                messages: &mut messages,
                effective_working_dir: &work_dir,
                workspace_is_set,
                // 留一份强引用：它让条目在 `/status` 里活到收尾结束（#1380）。
                per_flight: Arc::clone(&prepared.flight),
                temperature_override,
                model_override: prepared.client_model_override,
                use_executor_model: false,
                executor_model_override: prepared.executor_model_override,
                executor_api_base: prepared.executor_api_base,
                executor_api_key: prepared.executor_api_key,
                seed_override,
                long_term_memory: queue_deps.long_term_memory.clone(),
                job_id,
                conversation_id: conversation_id.as_str(),
                request_id,
                turn_allowed_tool_names: prepared.turn_allow,
                permission: prepared.permission,
                request_audit: std::sync::Arc::new(request_audit),
                process_handles: Arc::clone(&app.process_handles),
                tool_job_registry: Some(std::sync::Arc::clone(&app.tool_job_registry)),
                // `#1455` C 步：非流式路径**不**接暂停信号（自动续跑只对流式生效）。
                paused_reason_sink: None,
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
            cfg_snap: &prepared.cfg_snap,
            conversation_id: &conversation_id,
            messages: &mut messages,
            expected_revision,
            request_agent_role: request_agent_role.as_deref(),
            request_session_mode: request_session_mode.as_deref(),
            request_permission_level: request_permission_level.as_deref(),
        },
    )
    .await;
    JobOutcome::Json { ok, cancelled, err }
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
    request_session_mode: Option<&'a str>,
    request_permission_level: Option<&'a str>,
}

async fn finish_json_queued_job_after_turn(
    r: Result<crate::turn_runner::TurnOutcome, crate::agent::agent_turn::RunAgentTurnError>,
    p: JsonTurnFinishParams<'_>,
) -> (bool, bool, Option<String>) {
    match r {
        Ok(outcome) => match post_turn_web_prepare_and_save(PostTurnWebPrepareParams {
            app: p.app,
            queue_deps: p.queue_deps,
            cfg_snap: p.cfg_snap,
            conversation_id: p.conversation_id,
            messages: p.messages,
            expected_revision: p.expected_revision,
            request_agent_role: p.request_agent_role,
            request_session_mode: p.request_session_mode,
            request_permission_level: p.request_permission_level,
            structural_commit: outcome.commit,
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
