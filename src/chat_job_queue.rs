//! Web `/chat` / `/chat/stream` 的**进程内任务队列**：有界排队 + 并发上限，避免高并发时无界 `tokio::spawn`。
//!
//! - **多副本 / 跨进程重放**：需外部消息代理（Redis、SQS 等）与持久化；本模块仅单进程协调。
//! - **可观测**：`job_id` 写入日志；`/status` 暴露运行中任务数与近期任务摘要。

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use log::{debug, error, info};
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::AppState;
use crate::types::{CommandApprovalDecision, LLM_CANCELLED_ERROR, LlmSeedOverride, Message};

const RECENT_CAP: usize = 32;

/// 单条 `/chat` / `/chat/stream` 任务在跑 `run_agent_turn` 时，PER 相关状态的只读镜像（进程内、按 `job_id` 区分）。
///
/// **局限**：与浏览器「会话」无稳定绑定；同一客户端连续请求会得到不同 `job_id`。完整「本会话是否在规划重写」需会话级协议（如 `conversation_id`）再关联。
#[derive(Debug, Default)]
pub struct PerTurnFlight {
    /// 已追加「请重写终答规划」的 user 消息，正在等待下一轮模型输出。
    pub awaiting_plan_rewrite_model: AtomicBool,
    pub plan_rewrite_attempts: AtomicUsize,
    pub require_plan_in_final_content: AtomicBool,
}

impl PerTurnFlight {
    pub fn sync_from_per_coord(&self, p: &crate::agent::per_coord::PerCoordinator) {
        self.plan_rewrite_attempts
            .store(p.plan_rewrite_attempts_snapshot(), Ordering::Relaxed);
        self.require_plan_in_final_content
            .store(p.require_plan_in_final_flag_snapshot(), Ordering::Relaxed);
    }
}

/// `GET /status` 中 `per_active_jobs` 的单项（与 [`PerTurnFlight`] 原子字段对应）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PerFlightStatusEntry {
    pub job_id: u64,
    pub awaiting_plan_rewrite_model: bool,
    pub plan_rewrite_attempts: usize,
    pub require_plan_in_final_content: bool,
}

struct PerFlightJobGuard {
    queue: ChatJobQueue,
    job_id: u64,
}

impl Drop for PerFlightJobGuard {
    fn drop(&mut self) {
        self.queue.unregister_per_job_per_flight(self.job_id);
    }
}

/// 队列拒绝：有界通道已满（等待槽位过多）
#[derive(Debug, Clone, Copy)]
pub struct ChatQueueFull {
    pub max_pending: usize,
}

/// [`ChatJobQueue::try_submit_stream`] 的入参（避免长参数列表）。
pub struct StreamSubmitParams {
    pub job_id: u64,
    pub state: Arc<AppState>,
    pub conversation_id: String,
    pub messages: Vec<Message>,
    pub expected_revision: Option<u64>,
    pub work_dir: PathBuf,
    pub workspace_is_set: bool,
    pub temperature_override: Option<f32>,
    pub seed_override: LlmSeedOverride,
    pub sse_tx: mpsc::Sender<String>,
    pub web_approval_session: Option<WebApprovalSession>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatJobRecord {
    pub job_id: u64,
    pub kind: String,
    pub ok: bool,
    pub cancelled: bool,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_preview: Option<String>,
}

struct QueueMetrics {
    running: AtomicUsize,
    completed_ok: AtomicU64,
    completed_cancelled: AtomicU64,
    completed_err: AtomicU64,
}

impl Default for QueueMetrics {
    fn default() -> Self {
        Self {
            running: AtomicUsize::new(0),
            completed_ok: AtomicU64::new(0),
            completed_cancelled: AtomicU64::new(0),
            completed_err: AtomicU64::new(0),
        }
    }
}

enum QueuedChatJob {
    Stream {
        job_id: u64,
        state: Arc<AppState>,
        conversation_id: String,
        messages: Vec<Message>,
        expected_revision: Option<u64>,
        work_dir: PathBuf,
        workspace_is_set: bool,
        temperature_override: Option<f32>,
        seed_override: LlmSeedOverride,
        sse_tx: mpsc::Sender<String>,
        web_approval_session: Option<WebApprovalSession>,
    },
    Json {
        job_id: u64,
        state: Arc<AppState>,
        conversation_id: String,
        messages: Vec<Message>,
        expected_revision: Option<u64>,
        work_dir: PathBuf,
        workspace_is_set: bool,
        temperature_override: Option<f32>,
        seed_override: LlmSeedOverride,
        reply_tx: oneshot::Sender<Result<Vec<Message>, String>>,
    },
}

pub struct WebApprovalSession {
    pub session_id: String,
    pub approval_rx: mpsc::Receiver<CommandApprovalDecision>,
}

impl QueuedChatJob {
    fn job_id(&self) -> u64 {
        match self {
            QueuedChatJob::Stream { job_id, .. } | QueuedChatJob::Json { job_id, .. } => *job_id,
        }
    }
}

struct Inner {
    submit_tx: mpsc::Sender<QueuedChatJob>,
    max_concurrent: usize,
    max_pending: usize,
    next_job_id: AtomicU64,
    metrics: Arc<QueueMetrics>,
    recent: Arc<Mutex<VecDeque<ChatJobRecord>>>,
    /// 正在执行的队列任务的 PER 飞行快照（任务结束即移除）。
    active_per_flights: Arc<Mutex<HashMap<u64, Arc<PerTurnFlight>>>>,
}

/// `POST /chat` 与 `/chat/stream` 共用的进程内队列句柄（`Clone` 为轻量 `Arc`）。
#[derive(Clone)]
pub struct ChatJobQueue {
    inner: Arc<Inner>,
}

impl ChatJobQueue {
    pub fn new(max_concurrent: usize, max_pending: usize) -> Self {
        let max_concurrent = max_concurrent.max(1);
        let max_pending = max_pending.max(1);
        let (submit_tx, rx) = mpsc::channel::<QueuedChatJob>(max_pending);
        let sem = Arc::new(Semaphore::new(max_concurrent));
        let metrics = Arc::new(QueueMetrics::default());
        let recent = Arc::new(Mutex::new(VecDeque::with_capacity(RECENT_CAP)));

        let metrics_loop = metrics.clone();
        let recent_loop = recent.clone();
        tokio::spawn(dispatcher_loop(rx, sem, metrics_loop, recent_loop));

        Self {
            inner: Arc::new(Inner {
                submit_tx,
                max_concurrent,
                max_pending,
                next_job_id: AtomicU64::new(1),
                metrics,
                recent,
                active_per_flights: Arc::new(Mutex::new(HashMap::new())),
            }),
        }
    }

    pub fn max_concurrent(&self) -> usize {
        self.inner.max_concurrent
    }

    pub fn max_pending(&self) -> usize {
        self.inner.max_pending
    }

    pub fn next_job_id(&self) -> u64 {
        self.inner.next_job_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn running_count(&self) -> usize {
        self.inner.metrics.running.load(Ordering::SeqCst)
    }

    pub fn completed_ok(&self) -> u64 {
        self.inner.metrics.completed_ok.load(Ordering::SeqCst)
    }

    pub fn completed_err(&self) -> u64 {
        self.inner.metrics.completed_err.load(Ordering::SeqCst)
    }

    pub fn completed_cancelled(&self) -> u64 {
        self.inner
            .metrics
            .completed_cancelled
            .load(Ordering::SeqCst)
    }

    pub fn recent_jobs(&self) -> Vec<ChatJobRecord> {
        self.inner
            .recent
            .lock()
            .ok()
            .map(|g| g.iter().rev().cloned().collect())
            .unwrap_or_default()
    }

    fn begin_per_flight_job(&self, job_id: u64, flight: Arc<PerTurnFlight>) -> PerFlightJobGuard {
        if let Ok(mut g) = self.inner.active_per_flights.lock() {
            g.insert(job_id, flight);
        }
        PerFlightJobGuard {
            queue: self.clone(),
            job_id,
        }
    }

    fn unregister_per_job_per_flight(&self, job_id: u64) {
        if let Ok(mut g) = self.inner.active_per_flights.lock() {
            g.remove(&job_id);
        }
    }

    /// 当前正在执行的队列任务及其 PER 镜像（无运行中任务时为空 Vec）。
    pub fn active_per_jobs(&self) -> Vec<PerFlightStatusEntry> {
        let Ok(g) = self.inner.active_per_flights.lock() else {
            return Vec::new();
        };
        let mut v: Vec<PerFlightStatusEntry> = g
            .iter()
            .map(|(&job_id, flight)| PerFlightStatusEntry {
                job_id,
                awaiting_plan_rewrite_model: flight
                    .awaiting_plan_rewrite_model
                    .load(Ordering::Relaxed),
                plan_rewrite_attempts: flight.plan_rewrite_attempts.load(Ordering::Relaxed),
                require_plan_in_final_content: flight
                    .require_plan_in_final_content
                    .load(Ordering::Relaxed),
            })
            .collect();
        v.sort_by_key(|e| e.job_id);
        v
    }

    pub fn try_submit_stream(&self, p: StreamSubmitParams) -> Result<(), ChatQueueFull> {
        let StreamSubmitParams {
            job_id,
            state,
            conversation_id,
            messages,
            expected_revision,
            work_dir,
            workspace_is_set,
            temperature_override,
            seed_override,
            sse_tx,
            web_approval_session,
        } = p;
        let job = QueuedChatJob::Stream {
            job_id,
            state,
            conversation_id,
            messages,
            expected_revision,
            work_dir,
            workspace_is_set,
            temperature_override,
            seed_override,
            sse_tx,
            web_approval_session,
        };
        self.inner
            .submit_tx
            .try_send(job)
            .map_err(|_| ChatQueueFull {
                max_pending: self.inner.max_pending,
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_submit_json(
        &self,
        job_id: u64,
        state: Arc<AppState>,
        conversation_id: String,
        messages: Vec<Message>,
        expected_revision: Option<u64>,
        work_dir: PathBuf,
        workspace_is_set: bool,
        temperature_override: Option<f32>,
        seed_override: LlmSeedOverride,
        reply_tx: oneshot::Sender<Result<Vec<Message>, String>>,
    ) -> Result<(), ChatQueueFull> {
        let job = QueuedChatJob::Json {
            job_id,
            state,
            conversation_id,
            messages,
            expected_revision,
            work_dir,
            workspace_is_set,
            temperature_override,
            seed_override,
            reply_tx,
        };
        self.inner
            .submit_tx
            .try_send(job)
            .map_err(|_| ChatQueueFull {
                max_pending: self.inner.max_pending,
            })
    }
}

async fn dispatcher_loop(
    mut rx: mpsc::Receiver<QueuedChatJob>,
    sem: Arc<Semaphore>,
    metrics: Arc<QueueMetrics>,
    recent: Arc<Mutex<VecDeque<ChatJobRecord>>>,
) {
    while let Some(job) = rx.recv().await {
        // 先拿到并发令牌再 spawn，避免高积压时出现大量“已 spawn 但在等 permit”的任务。
        let permit = match sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => return,
        };
        let metrics = metrics.clone();
        let recent = recent.clone();
        metrics.running.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            let job_id = job.job_id();
            let _permit = permit;
            let start = Instant::now();
            let outcome = run_queued_job(job).await;
            let ms = start.elapsed().as_millis() as u64;
            metrics.running.fetch_sub(1, Ordering::SeqCst);

            let record = match outcome {
                JobOutcome::Stream { ok, cancelled, err } => {
                    if cancelled {
                        metrics.completed_cancelled.fetch_add(1, Ordering::SeqCst);
                    } else if ok {
                        metrics.completed_ok.fetch_add(1, Ordering::SeqCst);
                    } else {
                        metrics.completed_err.fetch_add(1, Ordering::SeqCst);
                    }
                    ChatJobRecord {
                        job_id,
                        kind: "stream".into(),
                        ok,
                        cancelled,
                        duration_ms: ms,
                        error_preview: err,
                    }
                }
                JobOutcome::Json { ok, cancelled, err } => {
                    if cancelled {
                        metrics.completed_cancelled.fetch_add(1, Ordering::SeqCst);
                    } else if ok {
                        metrics.completed_ok.fetch_add(1, Ordering::SeqCst);
                    } else {
                        metrics.completed_err.fetch_add(1, Ordering::SeqCst);
                    }
                    ChatJobRecord {
                        job_id,
                        kind: "json".into(),
                        ok,
                        cancelled,
                        duration_ms: ms,
                        error_preview: err,
                    }
                }
            };

            debug!(
                target: "crabmate",
                "chat 队列任务结束 job_id={} kind={} ok={} duration_ms={}",
                job_id,
                record.kind,
                record.ok,
                record.duration_ms
            );
            if record.cancelled {
                debug!(
                    target: "crabmate",
                    "chat 队列任务结束 job_id={} kind={} cancelled=true",
                    job_id,
                    record.kind
                );
            }

            if let Ok(mut g) = recent.lock() {
                g.push_back(record);
                while g.len() > RECENT_CAP {
                    g.pop_front();
                }
            }
        });
    }
}

enum JobOutcome {
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

fn is_user_cancelled_error(s: &str) -> bool {
    s.trim() == LLM_CANCELLED_ERROR
}

async fn run_queued_job(job: QueuedChatJob) -> JobOutcome {
    match job {
        QueuedChatJob::Stream {
            job_id,
            state,
            conversation_id,
            mut messages,
            expected_revision,
            work_dir,
            workspace_is_set,
            temperature_override,
            seed_override,
            sse_tx,
            web_approval_session,
        } => {
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
            let _per_guard = state
                .chat_queue
                .begin_per_flight_job(job_id, flight.clone());
            let out = Some(&sse_tx);
            let (web_tool_ctx, approval_session_id) = if let Some(session) = web_approval_session {
                (
                    Some(crate::tool_registry::WebToolRuntime {
                        out_tx: sse_tx.clone(),
                        approval_rx_shared: Arc::new(tokio::sync::Mutex::new(session.approval_rx)),
                        approval_request_guard: Arc::new(tokio::sync::Mutex::new(())),
                        persistent_allowlist_shared: Arc::new(tokio::sync::Mutex::new(
                            HashSet::new(),
                        )),
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
                tokio::spawn(async move {
                    tx_for_watch.closed().await;
                    cancel_for_watch.store(true, Ordering::SeqCst);
                })
            };
            let r = crate::run_agent_turn(crate::RunAgentTurnParams {
                client: &state.client,
                api_key: &state.api_key,
                cfg: &state.cfg,
                tools: &state.tools,
                messages: &mut messages,
                out,
                effective_working_dir: &work_dir,
                workspace_is_set,
                render_to_terminal: false,
                no_stream: false,
                cancel: Some(Arc::clone(&cancel)),
                per_flight: Some(flight),
                web_tool_ctx: web_tool_ctx.as_ref(),
                plain_terminal_stream: false,
                llm_backend: None,
                temperature_override,
                seed_override,
            })
            .await;
            cancel_watcher.abort();
            if let Some(session_id) = approval_session_id.as_deref() {
                state.approval_sessions.write().await.remove(session_id);
            }
            let cancelled_by_signal = cancel.load(Ordering::SeqCst);
            let (ok, cancelled, err) = match r {
                Ok(()) if cancelled_by_signal => {
                    info!(target: "crabmate", "chat stream 任务已取消 job_id={}", job_id);
                    (false, true, None)
                }
                Ok(()) => {
                    match state
                        .save_conversation_messages_if_revision(
                            conversation_id,
                            messages,
                            expected_revision,
                        )
                        .await
                    {
                        crate::SaveConversationOutcome::Saved => (true, false, None),
                        crate::SaveConversationOutcome::Conflict => {
                            let err_line = crate::save_outcome_to_stream_error_line(
                                crate::SaveConversationOutcome::Conflict,
                            )
                            .unwrap_or_else(|| {
                                crate::sse::encode_message(crate::sse::SsePayload::Error(
                                    crate::sse::SseErrorBody {
                                        error: "会话已被其他请求更新，请重试本次提问".to_string(),
                                        code: Some("CONVERSATION_CONFLICT".to_string()),
                                    },
                                ))
                            });
                            let _ = sse_tx.send(err_line).await;
                            (false, false, Some("conversation_conflict".to_string()))
                        }
                    }
                }
                Err(e) => {
                    let e_text = e.to_string();
                    if cancelled_by_signal || is_user_cancelled_error(&e_text) {
                        info!(
                            target: "crabmate",
                            "chat stream 任务已取消 job_id={} reason={}",
                            job_id,
                            e_text
                        );
                        (false, true, None)
                    } else {
                        error!(
                            target: "crabmate",
                            "chat stream 任务失败 job_id={} error={}",
                            job_id,
                            e_text
                        );
                        let err_line = crate::sse::encode_message(crate::sse::SsePayload::Error(
                            crate::sse::SseErrorBody {
                                error: "对话失败，请稍后重试".to_string(),
                                code: Some("INTERNAL_ERROR".to_string()),
                            },
                        ));
                        let _ = sse_tx.send(err_line).await;
                        (false, false, Some(truncate_chars(&e_text, 120)))
                    }
                }
            };
            drop(sse_tx);
            JobOutcome::Stream { ok, cancelled, err }
        }
        QueuedChatJob::Json {
            job_id,
            state,
            conversation_id,
            mut messages,
            expected_revision,
            work_dir,
            workspace_is_set,
            temperature_override,
            seed_override,
            reply_tx,
        } => {
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
            let _per_guard = state
                .chat_queue
                .begin_per_flight_job(job_id, flight.clone());
            let r = crate::run_agent_turn(crate::RunAgentTurnParams {
                client: &state.client,
                api_key: &state.api_key,
                cfg: &state.cfg,
                tools: &state.tools,
                messages: &mut messages,
                out: None,
                effective_working_dir: &work_dir,
                workspace_is_set,
                render_to_terminal: true,
                no_stream: false,
                cancel: None,
                per_flight: Some(flight),
                web_tool_ctx: None,
                plain_terminal_stream: false,
                llm_backend: None,
                temperature_override,
                seed_override,
            })
            .await;
            let (ok, cancelled, err) = match r {
                Ok(()) => {
                    match state
                        .save_conversation_messages_if_revision(
                            conversation_id,
                            messages.clone(),
                            expected_revision,
                        )
                        .await
                    {
                        crate::SaveConversationOutcome::Saved => {
                            let _ = reply_tx.send(Ok(messages));
                            (true, false, None)
                        }
                        crate::SaveConversationOutcome::Conflict => {
                            let _ = reply_tx.send(Err("CONVERSATION_CONFLICT".to_string()));
                            (false, false, Some("conversation_conflict".to_string()))
                        }
                    }
                }
                Err(e) => {
                    let e_text = e.to_string();
                    let cancelled = is_user_cancelled_error(&e_text);
                    if cancelled {
                        info!(
                            target: "crabmate",
                            "chat json 任务已取消 job_id={} reason={}",
                            job_id,
                            e_text
                        );
                    } else {
                        error!(
                            target: "crabmate",
                            "chat json 任务失败 job_id={} error={}",
                            job_id,
                            e_text
                        );
                    }
                    let prev = if cancelled {
                        None
                    } else {
                        Some(truncate_chars(&e_text, 120))
                    };
                    let _ = reply_tx.send(Err(e_text));
                    (false, cancelled, prev)
                }
            };
            JobOutcome::Json { ok, cancelled, err }
        }
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    let t: String = s.chars().take(max).collect();
    if t.len() < s.len() {
        format!("{}…", t)
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queue_accepts_config_bounds() {
        let q = ChatJobQueue::new(2, 4);
        assert_eq!(q.max_concurrent(), 2);
        assert_eq!(q.max_pending(), 4);
    }
}
