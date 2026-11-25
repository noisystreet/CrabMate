//! Web `/chat/stream` 队列任务执行体（从 `worker/mod.rs` 拆出以降低单文件行数）。

use log::{debug, info};

use super::super::stream_finish::{
    StreamEndedOnce, StreamJobOutcomeCtx, emit_stream_cancelled_terminal, emit_stream_ended_once,
    stream_job_outcome_after_agent_turn,
};
use super::super::{WebApprovalSession, WebChatJobEnvelope};
use super::JobOutcome;
use super::stream_job_setup::{StreamJobRuntime, StreamJobSetupParams, stream_job_setup_runtime};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// `run_stream_queued_job` 入参（[`WebChatJobEnvelope`] + SSE / 审批）。
pub(super) struct StreamQueuedJobParams {
    pub(super) envelope: WebChatJobEnvelope,
    pub(super) stream_event_tx: tokio::sync::mpsc::Sender<(u64, String)>,
    pub(super) web_approval_session: Option<WebApprovalSession>,
    pub(super) cancel: Arc<AtomicBool>,
}

pub(super) async fn run_stream_queued_job(p: StreamQueuedJobParams) -> JobOutcome {
    let job_id = p.envelope.job_id;
    info!(
        target: "crabmate",
        "chat stream 任务开始执行 job_id={}",
        job_id
    );
    debug!(
        target: "crabmate",
        "chat stream 执行上下文 job_id={} message_count={} last_user_preview={}",
        job_id,
        p.envelope.messages.len(),
        crate::redact::last_user_message_preview_for_log(&p.envelope.messages)
    );

    let queue_deps = p.envelope.queue_deps.clone();
    let (rt, cancel_watcher) = stream_job_setup_runtime(StreamJobSetupParams {
        envelope: &p.envelope,
        stream_event_tx: p.stream_event_tx,
        web_approval_session: p.web_approval_session,
        queue_deps: queue_deps.as_ref(),
        cancel: p.cancel,
    })
    .await;

    let WebChatJobEnvelope {
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
        llm_override,
        executor_llm_override,
        readonly_tool_ttl_cache_secs,
        github_token,
        client_sse_protocol,
        ..
    } = p.envelope;

    // 前置解析（配置 / 角色 / 工具 / 权限 / executor / per-flight）已由 setup 调
    // `prepare_queued_turn` 完成，这里只取用，不再二次解析。解构 `rt` 而不借用：
    // `prepared` 因此是拥有值，字段可直接 move 进 `TurnRequest`（与 JSON 路径同形）。
    let StreamJobRuntime {
        sse_tx,
        cancel,
        prepared,
        web_tool_ctx,
        approval_session_id,
    } = rt;

    // `#1455` C 步：本回合是否由**自动续跑**发起。必须在 `request_audit` 被 move 进
    // `TurnRequest` 之前取（续跑入队时把 source 设成 `auto_resume`）。
    let from_auto_resume = request_audit.is_auto_resume();

    // `#1455` C 步：暂停信号回传口。回合结束后从这里读「本回合是否因预算暂停」。
    // 用 `Arc<Mutex<_>>` 与 `run_agent_turn` 共享同一句柄——预算拒绝走 `Err`，
    // `TurnOutcome` 带不出来（见 `RunAgentTurnObs::paused_reason_sink` 的说明）。
    let paused_reason_sink: std::sync::Arc<
        std::sync::Mutex<Option<crate::cm_agent::task_state::TaskPauseReason>>,
    > = std::sync::Arc::new(std::sync::Mutex::new(None));
    // 落盘成功后的新 revision（入队续跑用；`SaveConversationOutcome` 不带它）。
    let saved_revision_sink: std::sync::Arc<std::sync::Mutex<Option<u64>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));

    let mut params = crate::TurnRequest::web(
        crate::RunAgentTurnSharedInputs {
            client: &queue_deps.client,
            api_key: prepared.api_key_turn.as_str(),
            cfg: &prepared.cfg_turn,
            tools: prepared.tools_for_job.as_slice(),
        },
        crate::RunAgentTurnSession {
            messages: &mut messages,
            effective_working_dir: &work_dir,
            workspace_is_set,
        },
        crate::AgentTurnLlmOverrides {
            temperature_override,
            model_override: prepared.client_model_override,
            use_executor_model: false,
            executor_model_override: prepared.executor_model_override,
            executor_api_base: prepared.executor_api_base,
            executor_api_key: prepared.executor_api_key,
            seed_override,
        },
        // 持有 `flight` 的强引用即在册（`#1380`）：它必须活到收尾结束。
        std::sync::Arc::clone(&prepared.flight),
        crate::WebTurnInputs {
            job_id,
            conversation_id: conversation_id.as_str(),
            request_id: request_id.clone(),
            request_audit: std::sync::Arc::new(request_audit),
            process_handles: std::sync::Arc::clone(&app.process_handles),
            // `TurnPermissionStages` 是 `Copy`，直接取用。
            permission: prepared.permission,
            long_term_memory: queue_deps.long_term_memory.clone(),
            turn_allowed_tool_names: prepared.turn_allow,
            tool_job_registry: Some(std::sync::Arc::clone(&app.tool_job_registry)),
            endpoint: crate::WebTurnEndpoint::Stream {
                out: &sse_tx,
                cancel: cancel.clone(),
                web_tool_ctx: web_tool_ctx
                    .as_ref()
                    .map(|c| c as &dyn crate::cm_approval::ApprovalBroker),
                paused_reason_sink: Some(std::sync::Arc::clone(&paused_reason_sink)),
            },
        },
    );
    // e2e 测试注入的自定义 LLM 后端（`None` 时使用默认 HTTP 后端）。
    params.transport.llm_backend = queue_deps.llm_backend;
    // 排队期间已 cancel：仍发过 setup 帧，但不再进 LLM/工具。
    let r = if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        // 未进 LLM/工具即被取消：合成一个空结构性提交，聚合保持 `Default`（全零 / 未截断）。
        Ok(crate::turn_runner::TurnOutcome {
            commit: crate::agent::model_context_view::StructuralCommit::default(),
            stats: crate::turn_runner::TurnStats::default(),
        })
    } else {
        queue_deps.turn_runner.run(params).await
    };

    cancel_watcher.abort();
    if let Some(session_id) = approval_session_id.as_deref() {
        app.approval_sessions.write().await.remove(session_id);
    }

    let cancelled_by_signal = cancel.load(std::sync::atomic::Ordering::SeqCst);
    // 层 1：`run_agent_turn` 通过 `PerTurnFlight` 记录本轮最后一次 assistant 输出是否被截断。
    let assistant_finish_truncated = prepared
        .flight
        .last_assistant_finish_truncated
        .load(std::sync::atomic::Ordering::Relaxed);
    let mut stream_ended_sent = StreamEndedOnce::default();
    let (ok, cancelled, err, stream_end_reason) =
        stream_job_outcome_after_agent_turn(StreamJobOutcomeCtx {
            r,
            cancelled_by_signal,
            truncated: assistant_finish_truncated,
            queue_deps: queue_deps.as_ref(),
            sse_tx: &sse_tx,
            flight: prepared.flight.as_ref(),
            job_id,
            request_id: request_id.clone(),
            messages: &mut messages,
            cfg_snap: &prepared.cfg_snap,
            app: &app,
            conversation_id: conversation_id.as_str(),
            expected_revision,
            request_agent_role: request_agent_role.as_deref(),
            request_session_mode: request_session_mode.as_deref(),
            request_permission_level: request_permission_level.as_deref(),
            stream_ended_sent: &mut stream_ended_sent,
            saved_revision_sink: Some(&saved_revision_sink),
        })
        .await;

    // `#1455` C 步：预算暂停后的自动续跑。放在 outcome 返回**之后**——此时落盘已完成，
    // `saved_revision_sink` 里是写回后的 revision（判据 5：状态定稿之后才排后续动作）。
    let paused_reason = paused_reason_sink.lock().ok().and_then(|g| *g);
    let saved_revision = saved_revision_sink.lock().ok().and_then(|g| *g);
    crate::chat_job_queue::auto_resume::maybe_auto_resume_next_turn(
        crate::chat_job_queue::auto_resume::AutoResumeCtx {
            queue_deps: &queue_deps,
            app: &app,
            cfg_snap: &prepared.cfg_snap,
            conversation_id: conversation_id.as_str(),
            paused_reason,
            saved_revision,
            // 用户刚点了停止：绝不替他再起一回合（取消优先于一切，含开关）。
            cancelled,
            // 本回合是否由续跑发起 —— 决定这次暂停要不要计入「连续无进展」。
            from_auto_resume,
            request_agent_role: request_agent_role.as_deref(),
            request_session_mode: request_session_mode.as_deref(),
            request_permission_level: request_permission_level.as_deref(),
            work_dir: work_dir.as_path(),
            workspace_is_set,
            temperature_override,
            seed_override,
            llm_override,
            executor_llm_override,
            readonly_tool_ttl_cache_secs,
            github_token,
            client_sse_protocol,
        },
    )
    .await;

    if cancelled {
        emit_stream_cancelled_terminal(&sse_tx, job_id, request_id.clone()).await;
    }
    // 先判不是防重复发帧（`emit_stream_ended_once` 自己守），而是跳过 tiktoken 计算。
    if !stream_ended_sent.is_sent() {
        let tiktoken_prompt_tokens =
            crate::agent::tiktoken_prompt_tokens::prompt_token_count_vendor_shaped_for_session(
                &prepared.cfg_snap,
                &messages,
            );
        // 兜底补发路径（主路径未发帧时才走到这里）：`missing` 不保证，按空处理——
        // 该路径拿不到主路径那份 verdict，硬凑会让「没证据」变成「有结论」（#1472）。
        emit_stream_ended_once(
            &sse_tx,
            job_id,
            stream_end_reason,
            Vec::new(),
            &mut stream_ended_sent,
            "chat_job_queue::stream stream_ended",
            tiktoken_prompt_tokens,
        )
        .await;
    }
    // 发完最后一帧就放手：桥接任务在通道关闭（⟺ 不可能再产帧）后自己摘除登记，因此这里
    // 不需要任何 hub 调用。`prepared` / `web_tool_ctx` 随函数返回 drop，这里只提前放掉
    // `sse_tx`（它可能是最后一个发送端）。
    drop(sse_tx);
    JobOutcome::Stream { ok, cancelled, err }
}
