//! Web `/chat` / `/chat/stream` 的**进程内任务队列**：有界排队 + 并发上限，避免高并发时无界 `tokio::spawn`。
//!
//! - **多副本 / 跨进程重放**：需外部消息代理（Redis、SQS 等）与持久化；本模块仅单进程协调。
//! - **可观测**：`job_id` 写入 tracing；`/status` 暴露运行中任务数与近期任务摘要。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::{Semaphore, mpsc, oneshot};
use tracing::{error, info};

use crate::AppState;
use crate::types::Message;

const RECENT_CAP: usize = 32;

/// 队列拒绝：有界通道已满（等待槽位过多）
#[derive(Debug, Clone, Copy)]
pub struct ChatQueueFull {
    pub max_pending: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatJobRecord {
    pub job_id: u64,
    pub kind: String,
    pub ok: bool,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_preview: Option<String>,
}

struct QueueMetrics {
    running: AtomicUsize,
    completed_ok: AtomicU64,
    completed_err: AtomicU64,
}

impl Default for QueueMetrics {
    fn default() -> Self {
        Self {
            running: AtomicUsize::new(0),
            completed_ok: AtomicU64::new(0),
            completed_err: AtomicU64::new(0),
        }
    }
}

enum QueuedChatJob {
    Stream {
        job_id: u64,
        state: Arc<AppState>,
        messages: Vec<Message>,
        work_dir: PathBuf,
        workspace_is_set: bool,
        sse_tx: mpsc::Sender<String>,
    },
    Json {
        job_id: u64,
        state: Arc<AppState>,
        messages: Vec<Message>,
        work_dir: PathBuf,
        workspace_is_set: bool,
        reply_tx: oneshot::Sender<Result<Vec<Message>, String>>,
    },
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

    pub fn recent_jobs(&self) -> Vec<ChatJobRecord> {
        self.inner
            .recent
            .lock()
            .ok()
            .map(|g| g.iter().rev().cloned().collect())
            .unwrap_or_default()
    }

    pub fn try_submit_stream(
        &self,
        job_id: u64,
        state: Arc<AppState>,
        messages: Vec<Message>,
        work_dir: PathBuf,
        workspace_is_set: bool,
        sse_tx: mpsc::Sender<String>,
    ) -> Result<(), ChatQueueFull> {
        let job = QueuedChatJob::Stream {
            job_id,
            state,
            messages,
            work_dir,
            workspace_is_set,
            sse_tx,
        };
        self.inner
            .submit_tx
            .try_send(job)
            .map_err(|_| ChatQueueFull {
                max_pending: self.inner.max_pending,
            })
    }

    pub fn try_submit_json(
        &self,
        job_id: u64,
        state: Arc<AppState>,
        messages: Vec<Message>,
        work_dir: PathBuf,
        workspace_is_set: bool,
        reply_tx: oneshot::Sender<Result<Vec<Message>, String>>,
    ) -> Result<(), ChatQueueFull> {
        let job = QueuedChatJob::Json {
            job_id,
            state,
            messages,
            work_dir,
            workspace_is_set,
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
        let sem = sem.clone();
        let metrics = metrics.clone();
        let recent = recent.clone();
        tokio::spawn(async move {
            let job_id = job.job_id();
            let permit = match sem.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };
            metrics.running.fetch_add(1, Ordering::SeqCst);
            let start = Instant::now();
            let outcome = run_queued_job(job).await;
            let ms = start.elapsed().as_millis() as u64;
            metrics.running.fetch_sub(1, Ordering::SeqCst);
            drop(permit);

            let record = match outcome {
                JobOutcome::Stream { ok, err } => {
                    if ok {
                        metrics.completed_ok.fetch_add(1, Ordering::SeqCst);
                    } else {
                        metrics.completed_err.fetch_add(1, Ordering::SeqCst);
                    }
                    ChatJobRecord {
                        job_id,
                        kind: "stream".into(),
                        ok,
                        duration_ms: ms,
                        error_preview: err,
                    }
                }
                JobOutcome::Json { ok, err } => {
                    if ok {
                        metrics.completed_ok.fetch_add(1, Ordering::SeqCst);
                    } else {
                        metrics.completed_err.fetch_add(1, Ordering::SeqCst);
                    }
                    ChatJobRecord {
                        job_id,
                        kind: "json".into(),
                        ok,
                        duration_ms: ms,
                        error_preview: err,
                    }
                }
            };

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
    Stream { ok: bool, err: Option<String> },
    Json { ok: bool, err: Option<String> },
}

async fn run_queued_job(job: QueuedChatJob) -> JobOutcome {
    match job {
        QueuedChatJob::Stream {
            job_id,
            state,
            mut messages,
            work_dir,
            workspace_is_set,
            sse_tx,
        } => {
            info!(job_id, "chat stream 任务开始执行");
            let out = Some(&sse_tx);
            let r = crate::run_agent_turn(
                &state.client,
                &state.api_key,
                &state.cfg,
                &state.tools,
                &mut messages,
                out,
                &work_dir,
                workspace_is_set,
                false,
                false,
            )
            .await;
            let (ok, err) = match r {
                Ok(()) => (true, None),
                Err(e) => {
                    error!(job_id, error = %e, "chat stream 任务失败");
                    let err_line = crate::sse_protocol::encode_message(
                        crate::sse_protocol::SsePayload::Error(crate::sse_protocol::SseErrorBody {
                            error: "对话失败，请稍后重试".to_string(),
                            code: Some("INTERNAL_ERROR".to_string()),
                        }),
                    );
                    let _ = sse_tx.send(err_line).await;
                    (false, Some(truncate_chars(&e.to_string(), 120)))
                }
            };
            drop(sse_tx);
            JobOutcome::Stream { ok, err }
        }
        QueuedChatJob::Json {
            job_id,
            state,
            mut messages,
            work_dir,
            workspace_is_set,
            reply_tx,
        } => {
            info!(job_id, "chat json 任务开始执行");
            let r = crate::run_agent_turn(
                &state.client,
                &state.api_key,
                &state.cfg,
                &state.tools,
                &mut messages,
                None,
                &work_dir,
                workspace_is_set,
                true,
                false,
            )
            .await;
            let (ok, err) = match r {
                Ok(()) => {
                    let _ = reply_tx.send(Ok(messages));
                    (true, None)
                }
                Err(e) => {
                    error!(job_id, error = %e, "chat json 任务失败");
                    let prev = truncate_chars(&e.to_string(), 120);
                    let _ = reply_tx.send(Err(e.to_string()));
                    (false, Some(prev))
                }
            };
            JobOutcome::Json { ok, err }
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
