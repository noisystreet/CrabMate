//! Web `/chat` / `/chat/stream` 的**进程内任务队列**：有界排队 + 并发上限，避免高并发时无界 `tokio::spawn`。
//!
//! - **多副本 / 跨进程重放**：需外部消息代理（Redis、SQS 等）与持久化；本模块仅单进程协调。
//! - **可观测**：`job_id` 写入日志；`/status` 暴露运行中任务数与近期任务摘要。用户停止须 **`POST /chat/stream/{job_id}/cancel`**（仅 abort SSE **不会**取消，以便 `stream_resume`）；取消且 SSE 仍可投递时补发 **`STREAM_CANCELLED`**（见 **`docs/SSE协议.md`**）。

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::config::{AgentConfig, LlmHttpAuthMode, SharedAgentConfig};
use crate::llm::ChatCompletionsBackend;
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::per_turn_flight::PerTurnFlight;
use crate::request_audit::WebRequestAudit;
use crate::sse::SseStreamHub;
use crate::types::{CommandApprovalDecision, LlmSeedOverride, Message, Tool};
use crate::web::WebChatJobAppFacet;
use log::debug;
use tokio::sync::{Semaphore, mpsc, oneshot};

mod stream_finish;
mod worker;

#[cfg(test)]
mod tests;

const RECENT_CAP: usize = 32;

/// Web 队列任务执行所需的**运行时句柄**（与 [`crate::AppState`] 中会话/上传等字段解耦，便于单测与依赖边界清晰）。
///
/// 会话落盘、审批会话表等经入队参数中的 [`WebChatJobAppFacet`] 完成（不再持有整包 [`crate::AppState`]）。
/// 回合执行经 [`crate::TurnRunner`] 注入（默认 [`crate::DefaultTurnRunner`]），**禁止** worker 直接调用 `run_agent_turn`。
#[derive(Clone)]
pub(crate) struct WebChatQueueDeps {
    pub cfg: SharedAgentConfig,
    pub api_key: String,
    pub client: reqwest::Client,
    pub tools: Vec<Tool>,
    pub chat_queue: ChatJobQueue,
    pub long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    pub sse_stream_hub: Arc<SseStreamHub>,
    /// e2e 测试注入的 LLM 后端（`None` 时使用默认 HTTP 后端）。
    ///
    /// 仅用于测试；生产路径无需设置。生命周期由 `Box::leak` 保障（`&'static`）。
    pub llm_backend: Option<&'static (dyn ChatCompletionsBackend + 'static)>,
    /// 回合执行面（默认转发 [`crate::run_agent_turn`]）。
    pub turn_runner: Arc<dyn crate::TurnRunner>,
}

/// Web `client_llm.llm_thinking_mode` 解析后的本回合 **`thinking`** 策略覆盖（不写服务端磁盘配置）。
/// 映射：`on`/`off` 写入 **`llm_bigmodel_thinking`** / **`llm_kimi_thinking_disabled`**；**DeepSeek 官方 `api_base`** 下由 [`crate::llm::vendor::DeepSeekVendor`] 转为请求体 **`thinking`** 与可选 **`reasoning_effort`**（见 DeepSeek [思考模式](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebClientLlmThinkingMode {
    /// 请求体显式开启：智谱等写 **`thinking: enabled`**；Kimi k2.5 不发送 **`disabled`**（即不关闭网关默认思考）。
    On,
    /// 请求体显式关闭：不写智谱 **`thinking`**；Kimi k2.5 发送 **`thinking: disabled`**（与其它关闭路径一致）。
    Off,
}

/// Web `POST /chat` / `/chat/stream` 请求体中可选的 **`client_llm`**：仅作用于**该次入队任务**，不写盘。
#[derive(Clone, Debug, Default)]
pub struct WebChatLlmOverride {
    pub api_base: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    /// 可选：覆盖本次任务的模型上下文窗口 token 上限（输入+输出），用于会话裁剪近似字符预算。
    pub llm_context_tokens: Option<u32>,
    /// 可选：覆盖 **`llm_bigmodel_thinking`** / **`llm_kimi_thinking_disabled`**（见 [`WebClientLlmThinkingMode`]）。
    pub llm_thinking_mode: Option<WebClientLlmThinkingMode>,
}

pub(super) fn resolve_web_llm_for_job(
    deps: &WebChatQueueDeps,
    cfg_snap: Arc<AgentConfig>,
    ov: Option<&WebChatLlmOverride>,
) -> (Arc<AgentConfig>, String) {
    let (mut cfg, key) = match ov {
        None => (cfg_snap, deps.api_key.clone()),
        Some(o) => {
            let mut c = (*cfg_snap).clone();
            let mut key = deps.api_key.clone();
            if let Some(ref x) = o.api_base {
                c.llm.api_base.clone_from(x);
            }
            // 注意：client_llm.model 不会覆盖 cfg.llm.model，以避免影响内部组件（如 L2 分类器）
            // 的模型选择。model 覆盖由调用方通过 model_override 参数单独传递给主对话循环。
            if let Some(ref x) = o.api_key {
                key.clone_from(x);
                c.llm.llm_http_auth_mode = LlmHttpAuthMode::Bearer;
            }
            if let Some(n) = o.llm_context_tokens {
                c.llm_sampling.llm_context_tokens = n;
            }
            if let Some(mode) = o.llm_thinking_mode {
                match mode {
                    WebClientLlmThinkingMode::On => {
                        c.llm_vendor_flags.llm_bigmodel_thinking = true;
                        c.llm_vendor_flags.llm_kimi_thinking_disabled = false;
                    }
                    WebClientLlmThinkingMode::Off => {
                        c.llm_vendor_flags.llm_bigmodel_thinking = false;
                        c.llm_vendor_flags.llm_kimi_thinking_disabled = true;
                    }
                }
            }
            (Arc::new(c), key)
        }
    };
    // 与 finalize / CLI 一致：运行时固定 ReAct（单 Agent 外循环）。
    {
        let mut c = (*cfg).clone();
        c.per_plan_policy.planner_executor_mode = crate::config::PlannerExecutorMode::SingleAgent;
        c.per_plan_policy.orchestration_profile = crate::config::OrchestrationProfile::ReAct;
        cfg = Arc::new(c);
    }
    (cfg, key)
}

/// 从 `executor_llm_override` 提取 executor 阶段专用的覆盖配置。
pub(super) fn resolve_executor_llm_for_job(
    deps: &WebChatQueueDeps,
    cfg_snap: Arc<AgentConfig>,
    ov: Option<&WebChatLlmOverride>,
) -> Option<(Arc<AgentConfig>, String)> {
    let o = ov?;
    let mut c = (*cfg_snap).clone();
    let mut key = deps.api_key.clone();
    let mut has_override = false;
    if let Some(ref x) = o.api_base {
        c.llm.api_base.clone_from(x);
        has_override = true;
    }
    if let Some(ref x) = o.model {
        c.llm.model.clone_from(x);
        has_override = true;
    }
    if let Some(ref x) = o.api_key {
        key.clone_from(x);
        c.llm.llm_http_auth_mode = LlmHttpAuthMode::Bearer;
        has_override = true;
    }
    if has_override {
        Some((Arc::new(c), key))
    } else {
        None
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

/// `POST /chat` 队列 worker 向 oneshot 返回的失败（与会话 revision 冲突区分）。
#[derive(Debug)]
pub enum ChatJsonJobFailure {
    ConversationConflict,
    Agent(crate::agent::agent_turn::RunAgentTurnError),
}

/// `POST /chat` 与 `/chat/stream` 入队任务共用的载荷（会话、消息、覆盖与审计）。
pub struct WebChatJobEnvelope {
    pub job_id: u64,
    /// 队列执行用 LLM/工具/hub 句柄（与 [`crate::AppState`] 会话字段分离）。
    pub queue_deps: Arc<WebChatQueueDeps>,
    /// 会话落盘 / 审批清理 / `ProcessHandles`（[`WebChatJobAppFacet`]，非整包 AppState）。
    pub app: WebChatJobAppFacet,
    pub conversation_id: String,
    pub messages: Vec<Message>,
    pub expected_revision: Option<u64>,
    /// 本请求 JSON 中的 `agent_role`（若有）；用于中途切换与落盘 `active_agent_role`。
    pub request_agent_role: Option<String>,
    /// 回合开始前服务端已持久化的当前角色（仅已有会话；新会话为 `None`）。
    pub persisted_active_agent_role: Option<String>,
    /// 本请求 JSON 中的 `session_mode`（若有）。
    pub request_session_mode: Option<String>,
    /// 回合开始前持久化的会话模式（空串/`None` 表示未设置）。
    pub persisted_active_session_mode: Option<String>,
    pub work_dir: PathBuf,
    pub workspace_is_set: bool,
    pub temperature_override: Option<f32>,
    pub seed_override: LlmSeedOverride,
    /// 可选：本任务覆盖 `api_base` / `model` / `api_key`（见 [`WebChatLlmOverride`]）。
    pub llm_override: Option<WebChatLlmOverride>,
    /// 可选：本任务覆盖执行阶段 `api_base` / `model` / `api_key`。
    pub executor_llm_override: Option<WebChatLlmOverride>,
    /// 可选：本任务覆盖 **`chat_queues_cache.readonly_tool_ttl_cache_secs`**（`None` 表示跟随服务端快照）。
    pub readonly_tool_ttl_cache_secs: Option<u64>,
    /// HTTP 审计上下文（客户端 IP、Bearer 指纹）；定时任务为占位。
    pub request_audit: WebRequestAudit,
    /// 与当次 HTTP **`x-request-id`** 同值；SSE `RUN_ERROR` 可带回客户端。
    pub request_id: Option<String>,
    /// 本请求 GitHub user token（头 / Cookie）；worker 内再挂入请求作用域。
    pub github_token: Option<String>,
    /// 客户端声明的 SSE 协议版本（`None` 兼容旧客户端 → v1）。
    ///
    /// 当前暂未在 worker 中用作 encoder 选择，待 Phase 3（前端 V2Parser）就绪后激活。
    #[allow(dead_code)]
    pub client_sse_protocol: Option<u8>,
}

/// [`ChatJobQueue::try_submit_json`] 的入参（[`WebChatJobEnvelope`] + JSON oneshot）。
pub struct JsonSubmitParams {
    pub envelope: WebChatJobEnvelope,
    pub reply_tx: oneshot::Sender<Result<Vec<Message>, ChatJsonJobFailure>>,
}

/// [`ChatJobQueue::try_submit_stream`] 的入参（[`WebChatJobEnvelope`] + SSE / 审批）。
pub struct StreamSubmitParams {
    pub envelope: WebChatJobEnvelope,
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

pub(super) enum QueuedChatJob {
    Stream {
        envelope: WebChatJobEnvelope,
        stream_event_tx: mpsc::Sender<(u64, String)>,
        web_approval_session: Option<WebApprovalSession>,
        cancel: Arc<AtomicBool>,
    },
    Json {
        envelope: WebChatJobEnvelope,
        reply_tx: oneshot::Sender<Result<Vec<Message>, ChatJsonJobFailure>>,
    },
}

pub struct WebApprovalSession {
    pub session_id: String,
    pub approval_rx: mpsc::Receiver<CommandApprovalDecision>,
}

impl QueuedChatJob {
    fn job_id(&self) -> u64 {
        match self {
            QueuedChatJob::Stream { envelope, .. } | QueuedChatJob::Json { envelope, .. } => {
                envelope.job_id
            }
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
    /// `/chat/stream` 协作取消：入队时登记，任务结束移除。Client「停止」走
    /// `POST /chat/stream/{job_id}/cancel` 置位；**不断** SSE 不断开即取消（否则无法 `stream_resume`）。
    stream_cancels: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    shutdown_triggered: Arc<AtomicBool>,
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
                stream_cancels: Arc::new(Mutex::new(HashMap::new())),
                shutdown_triggered: Arc::new(AtomicBool::new(false)),
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

    /// 触发队列关闭：拒绝新任务入队；已在途任务由 dispatcher 继续跑完。
    pub fn shutdown(&self) {
        self.inner.shutdown_triggered.store(true, Ordering::Release);
    }

    fn reject_if_shutdown(&self) -> Result<(), ChatQueueFull> {
        if self.inner.shutdown_triggered.load(Ordering::Acquire) {
            return Err(ChatQueueFull {
                max_pending: self.inner.max_pending,
            });
        }
        Ok(())
    }

    /// 为即将入队的流式 `job_id` 登记协作取消标志（与 worker 共用同一 `Arc`）。
    pub(crate) fn register_stream_cancel(&self, job_id: u64) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut g) = self.inner.stream_cancels.lock() {
            g.insert(job_id, Arc::clone(&flag));
        }
        flag
    }

    /// Client 显式停止：置位流式回合取消标志。任务未登记（已结束或不存在）返回 `false`。
    #[must_use]
    pub fn request_stream_cancel(&self, job_id: u64) -> bool {
        let Ok(g) = self.inner.stream_cancels.lock() else {
            return false;
        };
        let Some(flag) = g.get(&job_id) else {
            return false;
        };
        flag.store(true, Ordering::SeqCst);
        true
    }

    pub(crate) fn unregister_stream_cancel(&self, job_id: u64) {
        if let Ok(mut g) = self.inner.stream_cancels.lock() {
            g.remove(&job_id);
        }
    }

    pub fn try_submit_stream(&self, p: StreamSubmitParams) -> Result<(), ChatQueueFull> {
        self.reject_if_shutdown()?;
        let StreamSubmitParams {
            envelope,
            stream_event_tx,
            web_approval_session,
        } = p;
        let job_id = envelope.job_id;
        let cancel = self.register_stream_cancel(job_id);
        let job = QueuedChatJob::Stream {
            envelope,
            stream_event_tx,
            web_approval_session,
            cancel,
        };
        self.inner.submit_tx.try_send(job).map_err(|_| {
            self.unregister_stream_cancel(job_id);
            ChatQueueFull {
                max_pending: self.inner.max_pending,
            }
        })
    }

    pub fn try_submit_json(&self, p: JsonSubmitParams) -> Result<(), ChatQueueFull> {
        self.reject_if_shutdown()?;
        let JsonSubmitParams { envelope, reply_tx } = p;
        let job = QueuedChatJob::Json { envelope, reply_tx };
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
            Err(_) => {
                log::warn!(
                    target: "crabmate",
                    "队列调度器：信号量已关闭，仍有待处理任务被丢弃。请确保进程正常关闭。"
                );
                return;
            }
        };
        let metrics = metrics.clone();
        let recent = recent.clone();
        metrics.running.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            finish_dispatched_job(job, permit, metrics, recent).await;
        });
    }
    log::info!(target: "crabmate", "队列调度器：所有任务处理完毕，dispatcher 退出。");
}

fn bump_job_completion_metrics(metrics: &QueueMetrics, ok: bool, cancelled: bool) {
    if cancelled {
        metrics.completed_cancelled.fetch_add(1, Ordering::SeqCst);
    } else if ok {
        metrics.completed_ok.fetch_add(1, Ordering::SeqCst);
    } else {
        metrics.completed_err.fetch_add(1, Ordering::SeqCst);
    }
}

async fn finish_dispatched_job(
    job: QueuedChatJob,
    permit: tokio::sync::OwnedSemaphorePermit,
    metrics: Arc<QueueMetrics>,
    recent: Arc<Mutex<VecDeque<ChatJobRecord>>>,
) {
    let job_id = job.job_id();
    let stream_queue = match &job {
        QueuedChatJob::Stream { envelope, .. } => Some(envelope.queue_deps.chat_queue.clone()),
        QueuedChatJob::Json { .. } => None,
    };
    let _permit = permit;
    let start = Instant::now();
    let outcome = worker::run_queued_job(job).await;
    if let Some(q) = stream_queue {
        q.unregister_stream_cancel(job_id);
    }
    let ms = start.elapsed().as_millis() as u64;
    metrics.running.fetch_sub(1, Ordering::SeqCst);

    let kind = outcome.kind_label().to_string();
    let (ok, cancelled, err) = outcome.fields();
    bump_job_completion_metrics(&metrics, ok, cancelled);
    let record = ChatJobRecord {
        job_id,
        kind,
        ok,
        cancelled,
        duration_ms: ms,
        error_preview: err,
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
}
