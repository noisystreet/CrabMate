//! Web `/chat` / `/chat/stream` 的**进程内任务队列**：有界排队 + 并发上限，避免高并发时无界 `tokio::spawn`。
//!
//! - **多副本 / 跨进程重放**：需外部消息代理（Redis、SQS 等）与持久化；本模块仅单进程协调。
//! - **可观测**：`job_id` 写入日志；`/status` 暴露运行中任务数与近期任务摘要。流取消时 **`Receiver` drop** 打 **info**；取消且 SSE 仍可投递时补发 **`STREAM_CANCELLED`**（见 **`docs/SSE_PROTOCOL.md`**）。

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use log::{debug, error, info, warn};
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::AppState;
use crate::agent_errors::is_user_cancelled_run_agent_error;
use crate::agent_role_turn::{
    filter_tools_for_agent_role, persisted_agent_role_after_turn, turn_allow_for_web_or_cli_job,
};
use crate::config::{AgentConfig, LlmHttpAuthMode};
use crate::text_util::truncate_chars_with_ellipsis;
use crate::types::{CommandApprovalDecision, LlmSeedOverride, Message};

const RECENT_CAP: usize = 32;

/// Web `POST /chat` / `/chat/stream` 请求体中可选的 **`client_llm`**：仅作用于**该次入队任务**，不写盘。
#[derive(Clone, Debug, Default)]
pub struct WebChatLlmOverride {
    pub api_base: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
}

fn resolve_web_llm_for_job(
    state: &AppState,
    cfg_snap: Arc<AgentConfig>,
    ov: Option<&WebChatLlmOverride>,
) -> (Arc<AgentConfig>, String) {
    match ov {
        None => (cfg_snap, state.api_key.clone()),
        Some(o) => {
            let mut c = (*cfg_snap).clone();
            let mut key = state.api_key.clone();
            if let Some(ref x) = o.api_base {
                c.api_base.clone_from(x);
            }
            if let Some(ref x) = o.model {
                c.model.clone_from(x);
            }
            if let Some(ref x) = o.api_key {
                key.clone_from(x);
                c.llm_http_auth_mode = LlmHttpAuthMode::Bearer;
            }
            (Arc::new(c), key)
        }
    }
}

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

/// [`ChatJobQueue::try_submit_json`] 的入参（与 [`StreamSubmitParams`] 对称，不含 SSE / 审批）。
pub struct JsonSubmitParams {
    pub job_id: u64,
    pub state: Arc<AppState>,
    pub conversation_id: String,
    pub messages: Vec<Message>,
    pub expected_revision: Option<u64>,
    /// 本请求 JSON 中的 `agent_role`（若有）；用于中途切换与落盘 `active_agent_role`。
    pub request_agent_role: Option<String>,
    /// 回合开始前服务端已持久化的当前角色（仅已有会话；新会话为 `None`）。
    pub persisted_active_agent_role: Option<String>,
    pub work_dir: PathBuf,
    pub workspace_is_set: bool,
    pub temperature_override: Option<f32>,
    pub seed_override: LlmSeedOverride,
    /// 可选：本任务覆盖 `api_base` / `model` / `api_key`（见 [`WebChatLlmOverride`]）。
    pub llm_override: Option<WebChatLlmOverride>,
    pub reply_tx: oneshot::Sender<Result<Vec<Message>, String>>,
}

/// [`ChatJobQueue::try_submit_stream`] 的入参（避免长参数列表）。
pub struct StreamSubmitParams {
    pub job_id: u64,
    pub state: Arc<AppState>,
    pub conversation_id: String,
    pub messages: Vec<Message>,
    pub expected_revision: Option<u64>,
    pub request_agent_role: Option<String>,
    pub persisted_active_agent_role: Option<String>,
    pub work_dir: PathBuf,
    pub workspace_is_set: bool,
    pub temperature_override: Option<f32>,
    pub seed_override: LlmSeedOverride,
    /// 可选：本任务覆盖 `api_base` / `model` / `api_key`（见 [`WebChatLlmOverride`]）。
    pub llm_override: Option<WebChatLlmOverride>,
    /// HTTP SSE 层：每条为 **`(Last-Event-ID 序号, data 负载)`**（与 hub 环形缓冲一致）。
    pub stream_event_tx: mpsc::Sender<(u64, String)>,
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
        request_agent_role: Option<String>,
        persisted_active_agent_role: Option<String>,
        work_dir: PathBuf,
        workspace_is_set: bool,
        temperature_override: Option<f32>,
        seed_override: LlmSeedOverride,
        llm_override: Option<WebChatLlmOverride>,
        stream_event_tx: mpsc::Sender<(u64, String)>,
        web_approval_session: Option<WebApprovalSession>,
    },
    Json {
        job_id: u64,
        state: Arc<AppState>,
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
            request_agent_role,
            persisted_active_agent_role,
            work_dir,
            workspace_is_set,
            temperature_override,
            seed_override,
            llm_override,
            stream_event_tx,
            web_approval_session,
        } = p;
        let job = QueuedChatJob::Stream {
            job_id,
            state,
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
            stream_event_tx,
            web_approval_session,
        };
        self.inner
            .submit_tx
            .try_send(job)
            .map_err(|_| ChatQueueFull {
                max_pending: self.inner.max_pending,
            })
    }

    pub fn try_submit_json(&self, p: JsonSubmitParams) -> Result<(), ChatQueueFull> {
        let JsonSubmitParams {
            job_id,
            state,
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
            reply_tx,
        } = p;
        let job = QueuedChatJob::Json {
            job_id,
            state,
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

/// Web 队列：`run_agent_turn` 成功后的 LTM 异步索引、剥离注入与会话按 revision 落盘。
async fn post_turn_web_prepare_and_save(
    state: &AppState,
    cfg_snap: &Arc<AgentConfig>,
    conversation_id: &str,
    messages: &mut Vec<Message>,
    expected_revision: Option<u64>,
    request_agent_role: Option<&str>,
    persisted_active_agent_role: Option<&str>,
) -> crate::SaveConversationOutcome {
    let scope = conversation_id.to_string();
    let to_index = messages.clone();
    if let (Some(ltm), true) = (
        state.long_term_memory.as_ref(),
        cfg_snap.long_term_memory_enabled,
    ) {
        ltm.clone()
            .spawn_index_turn(Arc::clone(cfg_snap), scope, to_index);
    }
    crate::long_term_memory::strip_long_term_memory_injections(messages);
    crate::workspace_changelist::strip_workspace_changelist_injections(messages);
    let mut active_save =
        persisted_agent_role_after_turn(persisted_active_agent_role, request_agent_role);
    if active_save.is_none()
        && expected_revision.is_none()
        && let Some(id) = cfg_snap
            .default_agent_role_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        && cfg_snap.agent_roles.contains_key(id)
    {
        active_save = Some(id.to_string());
    }
    state
        .save_conversation_messages_if_revision(
            conversation_id.to_string(),
            messages.clone(),
            active_save.as_deref(),
            expected_revision,
        )
        .await
}

fn web_json_job_error_short_detail(
    e_text: &str,
    cancelled: bool,
    staged_invalid: bool,
) -> Option<String> {
    if cancelled {
        None
    } else if staged_invalid {
        Some("staged_plan_invalid".to_string())
    } else {
        Some(truncate_chars_with_ellipsis(e_text, 120))
    }
}

/// 流任务被取消且 **mpsc 仍有接收端** 时补发一条带 `code: STREAM_CANCELLED` 的控制面，便于前端与代理统一收尾（接收端已 drop 时仅 debug，避免误报）。
async fn emit_stream_cancelled_terminal(sse_tx: &mpsc::Sender<String>, job_id: u64) {
    if sse_tx.is_closed() {
        debug!(
            target: "crabmate",
            "stream 任务已取消且 SSE 已无接收端，跳过 STREAM_CANCELLED 帧 job_id={}",
            job_id
        );
        return;
    }
    let line =
        crate::sse::encode_message(crate::sse::SsePayload::Error(crate::sse::SseErrorBody {
            error: "流已取消".to_string(),
            code: Some(crate::types::SSE_STREAM_CANCELLED_CODE.to_string()),
            reason_code: None,
        }));
    if crate::sse::send_string_logged(
        sse_tx,
        line,
        "chat_job_queue::emit_stream_cancelled_terminal",
    )
    .await
    {
        debug!(
            target: "crabmate",
            "stream 已下发 STREAM_CANCELLED 控制帧 job_id={}",
            job_id
        );
    }
}

async fn run_queued_job(job: QueuedChatJob) -> JobOutcome {
    match job {
        QueuedChatJob::Stream {
            job_id,
            state,
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
            stream_event_tx,
            web_approval_session,
        } => {
            state.sse_stream_hub.register_job(job_id);
            let hub_bridge = state.sse_stream_hub.clone();
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
            let _per_guard = state
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
                let g = state.cfg.read().await;
                std::sync::Arc::new(g.clone())
            };
            let (cfg_turn, api_key_turn) =
                resolve_web_llm_for_job(&state, cfg_snap.clone(), llm_override.as_ref());
            let turn_allow = turn_allow_for_web_or_cli_job(
                &cfg_turn,
                persisted_active_agent_role.as_deref(),
                request_agent_role.as_deref(),
            );
            let tools_for_job =
                filter_tools_for_agent_role(&state.tools, turn_allow.as_ref().map(|a| a.as_ref()));
            let r = crate::run_agent_turn(crate::RunAgentTurnParams::web_chat_stream(
                &state.client,
                api_key_turn.as_str(),
                &cfg_turn,
                tools_for_job.as_slice(),
                &mut messages,
                &work_dir,
                workspace_is_set,
                Arc::clone(&cancel),
                flight,
                web_tool_ctx.as_ref(),
                temperature_override,
                seed_override,
                state.long_term_memory.clone(),
                &conversation_id,
                &sse_tx,
                turn_allow,
            ))
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
                    match post_turn_web_prepare_and_save(
                        &state,
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
                            if let Some(new_rev) = state
                                .load_conversation_seed(&conversation_id)
                                .await
                                .and_then(|s| s.expected_revision)
                            {
                                let line = crate::sse::encode_message(
                                    crate::sse::SsePayload::ConversationSaved {
                                        saved: crate::sse::ConversationSavedBody {
                                            revision: new_rev,
                                        },
                                    },
                                );
                                let _ = crate::sse::send_string_logged(
                                    &sse_tx,
                                    line,
                                    "chat_job_queue::stream conversation_saved",
                                )
                                .await;
                            }
                            (true, false, None)
                        }
                        crate::SaveConversationOutcome::Conflict => {
                            let err_line = crate::conversation_conflict_sse_line();
                            let _ = crate::sse::send_string_logged(
                                &sse_tx,
                                err_line,
                                "chat_job_queue::stream conversation_conflict",
                            )
                            .await;
                            (false, false, Some("conversation_conflict".to_string()))
                        }
                    }
                }
                Err(e) => {
                    let e_text = e.to_string();
                    if cancelled_by_signal || is_user_cancelled_run_agent_error(&e_text) {
                        info!(
                            target: "crabmate",
                            "chat stream 任务已取消 job_id={} reason={}",
                            job_id,
                            e_text
                        );
                        (false, true, None)
                    } else if crate::agent::plan_artifact::is_staged_plan_invalid_run_agent_turn_error(
                        &e_text,
                    ) {
                        warn!(
                            target: "crabmate",
                            "chat stream 任务结束（staged_plan_invalid 前缀错误，多为旧服务端或非常规路径） job_id={} detail={}",
                            job_id,
                            e_text
                        );
                        (false, false, Some("staged_plan_invalid".to_string()))
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
                                reason_code: None,
                            },
                        ));
                        let _ = crate::sse::send_string_logged(
                            &sse_tx,
                            err_line,
                            "chat_job_queue::stream internal_error",
                        )
                        .await;
                        (false, false, Some(truncate_chars_with_ellipsis(&e_text, 120)))
                    }
                }
            };
            if cancelled {
                emit_stream_cancelled_terminal(&sse_tx, job_id).await;
            }
            let end_reason = if cancelled { "cancelled" } else { "completed" };
            let end_line = crate::sse::encode_message(crate::sse::SsePayload::StreamEnded {
                ended: crate::sse::StreamEndedBody {
                    job_id,
                    reason: end_reason.to_string(),
                },
            });
            let _ = crate::sse::send_string_logged(
                &sse_tx,
                end_line,
                "chat_job_queue::stream stream_ended",
            )
            .await;
            drop(sse_tx);
            state.sse_stream_hub.remove_job(job_id);
            JobOutcome::Stream { ok, cancelled, err }
        }
        QueuedChatJob::Json {
            job_id,
            state,
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
            let cfg_snap = {
                let g = state.cfg.read().await;
                std::sync::Arc::new(g.clone())
            };
            let (cfg_turn, api_key_turn) =
                resolve_web_llm_for_job(&state, cfg_snap.clone(), llm_override.as_ref());
            let turn_allow = turn_allow_for_web_or_cli_job(
                &cfg_turn,
                persisted_active_agent_role.as_deref(),
                request_agent_role.as_deref(),
            );
            let tools_for_job =
                filter_tools_for_agent_role(&state.tools, turn_allow.as_ref().map(|a| a.as_ref()));
            let r = crate::run_agent_turn(crate::RunAgentTurnParams::web_chat_json(
                &state.client,
                api_key_turn.as_str(),
                &cfg_turn,
                tools_for_job.as_slice(),
                &mut messages,
                &work_dir,
                workspace_is_set,
                flight,
                temperature_override,
                seed_override,
                state.long_term_memory.clone(),
                &conversation_id,
                turn_allow,
            ))
            .await;
            let (ok, cancelled, err) = match r {
                Ok(()) => {
                    match post_turn_web_prepare_and_save(
                        &state,
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
                                .send(Err("CONVERSATION_CONFLICT".to_string()))
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
                    let e_text = e.to_string();
                    let cancelled = is_user_cancelled_run_agent_error(&e_text);
                    let staged_invalid =
                        crate::agent::plan_artifact::is_staged_plan_invalid_run_agent_turn_error(
                            &e_text,
                        );
                    if cancelled {
                        info!(
                            target: "crabmate",
                            "chat json 任务已取消 job_id={} reason={}",
                            job_id,
                            e_text
                        );
                    } else if staged_invalid {
                        warn!(
                            target: "crabmate",
                            "chat json 任务结束（分阶段规划解析失败） job_id={} detail={}",
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
                    let prev = web_json_job_error_short_detail(&e_text, cancelled, staged_invalid);
                    if reply_tx.send(Err(e_text)).is_err() {
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
