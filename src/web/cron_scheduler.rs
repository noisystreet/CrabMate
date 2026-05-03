//! `serve` 内基于 **`tokio-cron-scheduler`** 的定时对话：配置见顶层 **`[[scheduled_agent_task]]`**。

use std::sync::Arc;

use log::{error, info, warn};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::chat_job_queue;
use crate::config::ScheduledAgentTask;
use crate::types::LlmSeedOverride;

use super::app_state::AppState;
use super::audit;
use super::chat_handlers::{normalize_agent_role, prepare_json_chat_enqueue};

/// 启动 cron：按当前配置注册任务并 `tokio::spawn` 跑调度器主循环。
pub(crate) fn spawn_serve_cron_scheduler(state: Arc<AppState>, tasks: Vec<ScheduledAgentTask>) {
    if tasks.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let sched = match JobScheduler::new().await {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    target: "crabmate",
                    "定时任务调度器创建失败（跳过 scheduled_agent_task） err={e}"
                );
                return;
            }
        };
        for t in tasks {
            let label = t.id.clone();
            let schedule = t.schedule.clone();
            let st = state.clone();
            let task = t.clone();
            let job = match Job::new_async(schedule.as_str(), move |_jid, _lock| {
                let st = st.clone();
                let task = task.clone();
                Box::pin(async move {
                    run_scheduled_json_turn(st, task).await;
                })
            }) {
                Ok(j) => j,
                Err(e) => {
                    warn!(
                        target: "crabmate",
                        "跳过定时任务 id={} schedule={} err={}",
                        label,
                        schedule,
                        e
                    );
                    continue;
                }
            };
            let sched_ref = sched.clone();
            if let Err(e) = sched_ref.add(job).await {
                warn!(
                    target: "crabmate",
                    "定时任务注册失败 id={} schedule={} err={}",
                    label,
                    schedule,
                    e
                );
            } else {
                info!(
                    target: "crabmate",
                    "定时任务已注册 id={} schedule={}",
                    label,
                    schedule
                );
            }
        }
        if let Err(e) = sched.start().await {
            error!(
                target: "crabmate",
                "tokio-cron-scheduler start 失败 err={e}"
            );
        }
    });
}

async fn run_scheduled_json_turn(state: Arc<AppState>, task: ScheduledAgentTask) {
    let agent_role: Option<String> = match normalize_agent_role(task.agent_role.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            warn!(
                target: "crabmate",
                "定时任务跳过 id={} agent_role 非法：{}",
                task.id,
                e
            );
            return;
        }
    };
    let conversation_id = if task.new_conversation {
        state.next_conversation_id()
    } else {
        match task.conversation_id.as_deref() {
            Some(s) => s.to_string(),
            None => {
                warn!(
                    target: "crabmate",
                    "定时任务跳过 id={}：缺少 conversation_id 且未设 new_conversation",
                    task.id
                );
                return;
            }
        }
    };
    let prepared = match prepare_json_chat_enqueue(
        &state,
        task.message.as_str(),
        None,
        &[],
        agent_role.clone(),
        conversation_id.clone(),
    )
    .await
    {
        Ok(p) => p,
        Err((status, body)) => {
            warn!(
                target: "crabmate",
                "定时任务 id={} 准备入队失败 http={} code={} msg={}",
                task.id,
                status,
                body.0.code,
                body.0.message
            );
            return;
        }
    };
    let job_id = state.chat.chat_queue.next_job_id();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    info!(
        target: "crabmate",
        "定时任务入队 id={} job_id={} conversation_id={} user_preview={}",
        task.id,
        job_id,
        prepared.conversation_id,
        crate::redact::preview_chars(
            &prepared.msg_for_log,
            crate::redact::MESSAGE_LOG_PREVIEW_CHARS
        )
    );
    if let Err(e) = state
        .chat
        .chat_queue
        .try_submit_json(chat_job_queue::JsonSubmitParams {
            job_id,
            queue_deps: state.chat.chat_queue_job_deps.clone(),
            app: state.clone(),
            conversation_id: prepared.conversation_id,
            messages: prepared.turn_seed.messages,
            expected_revision: prepared.turn_seed.expected_revision,
            request_agent_role: agent_role,
            persisted_active_agent_role: prepared.turn_seed.persisted_active_agent_role,
            work_dir: prepared.work_dir,
            workspace_is_set: prepared.workspace_is_set,
            temperature_override: None,
            seed_override: LlmSeedOverride::FromConfig,
            llm_override: None,
            executor_llm_override: None,
            execution_mode_override: None,
            request_audit: audit::WebRequestAudit::scheduled_placeholder(),
            reply_tx,
        })
    {
        warn!(
            target: "crabmate",
            "定时任务 id={} 队列已满（max_pending={}）",
            task.id,
            e.max_pending
        );
        return;
    }
    tokio::spawn(async move {
        let outcome = reply_rx.await;
        match outcome {
            Ok(Ok(_msgs)) => {
                info!(
                    target: "crabmate",
                    "定时任务完成 id={} job_id={}",
                    task.id,
                    job_id,
                );
            }
            Ok(Err(chat_job_queue::ChatJsonJobFailure::ConversationConflict)) => {
                warn!(
                    target: "crabmate",
                    "定时任务失败 id={} job_id={} conversation revision 冲突",
                    task.id,
                    job_id
                );
            }
            Ok(Err(chat_job_queue::ChatJsonJobFailure::Agent(e))) => {
                warn!(
                    target: "crabmate",
                    "定时任务 Agent 失败 id={} job_id={} {}",
                    task.id,
                    job_id,
                    e.diag_log_kv()
                );
            }
            Err(_) => {}
        }
    });
}
