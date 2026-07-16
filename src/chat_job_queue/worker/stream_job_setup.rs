//! `run_stream_queued_job` 的 SSE 桥接与执行上下文准备。

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use log::info;
use tokio::sync::mpsc;

use crate::agent_role_turn::{filter_tools_for_agent_role, turn_allow_for_web_or_cli_job};

use super::super::WebChatQueueDeps;
use super::super::{
    PerTurnFlight, WebApprovalSession, WebChatJobEnvelope, resolve_executor_llm_for_job,
    resolve_web_llm_for_job,
};

pub(super) struct StreamJobRuntime {
    pub sse_tx: mpsc::Sender<String>,
    pub cancel: Arc<AtomicBool>,
    pub flight: Arc<PerTurnFlight>,
    pub cfg_turn: Arc<crate::config::AgentConfig>,
    pub api_key_turn: String,
    pub tools_for_job: Arc<Vec<crate::types::Tool>>,
    pub turn_allow: Option<Arc<std::collections::HashSet<String>>>,
    pub executor_api_base: Option<String>,
    pub executor_api_key: Option<String>,
    pub executor_model_override: Option<String>,
    pub web_tool_ctx: Option<crate::tool_registry::WebToolRuntime>,
    pub approval_session_id: Option<String>,
}

pub(super) struct StreamJobSetupParams<'a> {
    pub envelope: &'a WebChatJobEnvelope,
    pub stream_event_tx: mpsc::Sender<(u64, String)>,
    pub web_approval_session: Option<WebApprovalSession>,
    pub queue_deps: &'a WebChatQueueDeps,
}

pub(super) async fn stream_job_setup_runtime(
    p: StreamJobSetupParams<'_>,
) -> (StreamJobRuntime, tokio::task::JoinHandle<()>) {
    let job_id = p.envelope.job_id;
    p.queue_deps.sse_stream_hub.register_job(job_id);
    let hub_bridge = p.queue_deps.sse_stream_hub.clone();
    let http_tx = p.stream_event_tx.clone();
    let (sse_tx, mut sse_rx) = mpsc::channel::<String>(1024);
    let bridge_job = job_id;
    tokio::spawn(async move {
        while let Some(line) = sse_rx.recv().await {
            if let Some(pair) = hub_bridge.publish(bridge_job, line) {
                let _ = http_tx.send(pair).await;
            }
        }
    });

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

    // AG-UI v2：标记回合开始与思维链生命周期
    if p.queue_deps.sse_encoder.format_version() == 2 {
        crate::sse::send_run_started_sse(
            &sse_tx,
            "main",              // thread_id
            &job_id.to_string(), // run_id
            p.queue_deps.sse_encoder.as_ref(),
        )
        .await;
        crate::sse::send_reasoning_message_start_sse(
            &sse_tx,
            "reasoning",
            p.queue_deps.sse_encoder.as_ref(),
        )
        .await;
    }

    let (web_tool_ctx, approval_session_id) =
        stream_job_web_tool_ctx(p.web_approval_session, &sse_tx);

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_watcher =
        stream_job_spawn_cancel_watcher(sse_tx.clone(), Arc::clone(&cancel), job_id);

    let flight = Arc::new(PerTurnFlight::default());
    let _per_guard = p
        .queue_deps
        .chat_queue
        .begin_per_flight_job(job_id, flight.clone());

    let cfg_snap = {
        let g = p.queue_deps.cfg.read().await;
        Arc::new(g.clone())
    };
    let (mut cfg_turn, api_key_turn) = resolve_web_llm_for_job(
        p.queue_deps,
        cfg_snap.clone(),
        p.envelope.llm_override.as_ref(),
    );
    if let Some(secs) = p.envelope.readonly_tool_ttl_cache_secs {
        let mut c = (*cfg_turn).clone();
        c.chat_queues_cache.readonly_tool_ttl_cache_secs = secs;
        cfg_turn = Arc::new(c);
    }
    let turn_allow = turn_allow_for_web_or_cli_job(
        &cfg_turn,
        p.envelope.persisted_active_agent_role.as_deref(),
        p.envelope.request_agent_role.as_deref(),
    );
    let tools_for_job = Arc::new(filter_tools_for_agent_role(
        &p.queue_deps.tools,
        turn_allow.as_ref().map(|a| a.as_ref()),
    ));
    let (executor_api_base, executor_api_key, executor_model_override) =
        stream_job_resolve_executor_llm(p.queue_deps, cfg_turn.clone(), p.envelope);

    let runtime = StreamJobRuntime {
        sse_tx,
        cancel,
        flight,
        cfg_turn,
        api_key_turn,
        tools_for_job,
        turn_allow,
        executor_api_base,
        executor_api_key,
        executor_model_override,
        web_tool_ctx,
        approval_session_id,
    };
    (runtime, cancel_watcher)
}

fn stream_job_web_tool_ctx(
    web_approval_session: Option<WebApprovalSession>,
    sse_tx: &mpsc::Sender<String>,
) -> (Option<crate::tool_registry::WebToolRuntime>, Option<String>) {
    if let Some(session) = web_approval_session {
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
    }
}

fn stream_job_spawn_cancel_watcher(
    sse_tx: mpsc::Sender<String>,
    cancel: Arc<AtomicBool>,
    job_id: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        sse_tx.closed().await;
        cancel.store(true, Ordering::SeqCst);
        info!(
            target: "crabmate",
            "chat stream SSE 接收端关闭，已请求取消 job_id={}",
            job_id
        );
    })
}

fn stream_job_resolve_executor_llm(
    queue_deps: &WebChatQueueDeps,
    cfg_turn: Arc<crate::config::AgentConfig>,
    envelope: &WebChatJobEnvelope,
) -> (Option<String>, Option<String>, Option<String>) {
    let executor_override = resolve_executor_llm_for_job(
        queue_deps,
        cfg_turn.clone(),
        envelope.executor_llm_override.as_ref(),
    );
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
            (base, Some(executor_key), model)
        }
        None => (None, None, None),
    }
}
