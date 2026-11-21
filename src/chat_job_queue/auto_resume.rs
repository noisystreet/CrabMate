//! `#1455` C 步：回合收尾处的自动续跑决策与入队。
//!
//! ## 为什么在这里而不是回合内
//!
//! 判据 5：任何「为同一实体排后续动作」的事件都必须在该实体**状态定稿之后**发出。
//! 本回合的会话 revision 在 `post_turn_web_prepare_and_save` 里写回，所以入队点必须在
//! 那次写回**之后**——否则 `expected_revision` 是旧值，续跑一开跑就 `CONVERSATION_CONFLICT`。
//!
//! ## 为什么走 JSON 队列而不是 stream
//!
//! 续跑回合**没有客户端在监听 SSE**。走 stream 形态会拿到一个无人接收的
//! `stream_event_tx`，`send` 失败可能中止回合。JSON 形态（oneshot）天然无 SSE。
//!
//! ## 失败一律放弃，不降级
//!
//! 拿不到「写回后的 revision」时不退化成 `expected_revision: None`——那等于放弃 CAS，
//! 会覆盖期间发生的并发更新。宁可这一轮不续跑。

use std::path::Path;
use std::sync::{Arc, MutexGuard};

use crate::chat_job_queue::{
    ChatJsonJobFailure, JsonSubmitParams, WebChatJobEnvelope, WebChatLlmOverride, WebChatQueueDeps,
};
use crate::cm_agent::run_epoch::RunEpoch;
use crate::cm_agent::task_state::{
    AutoResumeDecision, TaskPauseReason, decide_auto_resume,
};
use crate::cm_config::AgentConfig;
use crate::cm_types::Message;
use crate::types::LlmSeedOverride;
use crate::web::WebChatJobAppFacet;

/// 收尾处决策所需的上下文（字段多，用结构体而不是长形参）。
pub(super) struct AutoResumeCtx<'a> {
    pub(super) queue_deps: &'a Arc<WebChatQueueDeps>,
    pub(super) app: &'a WebChatJobAppFacet,
    pub(super) cfg_snap: &'a Arc<AgentConfig>,
    pub(super) conversation_id: &'a str,
    /// 本回合**因何暂停**（`None` = 正常完成或非预算中止）。
    pub(super) paused_reason: Option<TaskPauseReason>,
    /// 落盘成功后的**新** revision（`None` = 未成功落盘，不得续跑）。
    pub(super) saved_revision: Option<u64>,
    pub(super) request_agent_role: Option<&'a str>,
    pub(super) request_session_mode: Option<&'a str>,
    pub(super) request_permission_level: Option<&'a str>,
    pub(super) work_dir: &'a Path,
    pub(super) workspace_is_set: bool,
    pub(super) temperature_override: Option<f32>,
    pub(super) seed_override: LlmSeedOverride,
    /// 沿用本回合的客户端 LLM 覆盖：续跑是「同一请求的继续」，换密钥可能落到不同配额。
    pub(super) llm_override: Option<WebChatLlmOverride>,
    pub(super) executor_llm_override: Option<WebChatLlmOverride>,
    pub(super) readonly_tool_ttl_cache_secs: Option<u64>,
    pub(super) github_token: Option<String>,
    pub(super) client_sse_protocol: Option<u8>,
}

/// 账本句柄。**锁中毒也要能继续**：续跑是增强功能，不该因一次 panic 永久失效。
fn epoch(app: &WebChatJobAppFacet) -> MutexGuard<'_, RunEpoch> {
    app.run_epoch.lock().unwrap_or_else(|p| p.into_inner())
}

/// 判定并按需入队下一回合。
///
/// **默认配置下第一行即返回**：`autonomy_enabled` / `auto_resume` 任一为 false 就零 IO、
/// 零副作用——这是「关闭时与今天逐字一致」的保证点。
pub(super) async fn maybe_auto_resume_next_turn(ctx: AutoResumeCtx<'_>) {
    let autonomy = &ctx.cfg_snap.autonomy;
    if !autonomy.enabled || !autonomy.auto_resume {
        return;
    }

    let Some(reason) = ctx.paused_reason else {
        // 有进展（或非预算中止）：清掉「连续同因」计数，但保留轮数。
        epoch(ctx.app).clear_progress(ctx.conversation_id);
        return;
    };

    let Some(new_rev) = ctx.saved_revision else {
        tracing::warn!(
            target: "crabmate::autonomy",
            conversation_id = %ctx.conversation_id,
            "auto-resume skipped: conversation not saved, no usable revision"
        );
        return;
    };

    let (resumed_turns, consecutive) = {
        let mut e = epoch(ctx.app);
        e.record_pause(ctx.conversation_id, reason);
        let s = e.snapshot(ctx.conversation_id);
        (s.resumed_turns, s.consecutive_same_reason)
    };

    match decide_auto_resume(autonomy, Some(reason), resumed_turns, consecutive, false) {
        AutoResumeDecision::Go => {}
        AutoResumeDecision::Stop(why) => {
            tracing::info!(
                target: "crabmate::autonomy",
                conversation_id = %ctx.conversation_id,
                reason = reason.as_str(),
                stop_reason = why.as_str(),
                "auto-resume stopped"
            );
            return;
        }
    }

    match submit_resumed_turn(&ctx, new_rev).await {
        Ok(job_id) => tracing::info!(
            target: "crabmate::autonomy",
            conversation_id = %ctx.conversation_id,
            reason = reason.as_str(),
            job_id,
            "auto-resume submitted"
        ),
        Err(e) => tracing::warn!(
            target: "crabmate::autonomy",
            conversation_id = %ctx.conversation_id,
            "auto-resume submit failed: {e}"
        ),
    }
}

/// 构造并提交续跑回合。返回新 job_id。
async fn submit_resumed_turn(ctx: &AutoResumeCtx<'_>, new_rev: u64) -> Result<u64, String> {
    let Some(seed) = ctx
        .app
        .conversation
        .load_conversation_seed(ctx.conversation_id)
        .await
    else {
        return Err("conversation disappeared after save".to_string());
    };

    // 用**权威历史**（store 里的，已剥离注入）而不是本回合缓冲：后者已 strip 且含本轮临时状态。
    let mut messages = seed.messages;
    messages.push(auto_resume_instruction());

    let queue = &ctx.queue_deps.chat_queue;
    let job_id = queue.next_job_id();
    let envelope = WebChatJobEnvelope {
        job_id,
        queue_deps: Arc::clone(ctx.queue_deps),
        app: ctx.app.clone(),
        conversation_id: ctx.conversation_id.to_string(),
        messages,
        // 关键：用**写回后**的 revision，CAS 必然命中。
        expected_revision: Some(new_rev),
        request_agent_role: ctx.request_agent_role.map(str::to_string),
        persisted_active_agent_role: seed.persisted_active_agent_role,
        request_session_mode: ctx.request_session_mode.map(str::to_string),
        persisted_active_session_mode: seed.persisted_active_session_mode,
        request_permission_level: ctx.request_permission_level.map(str::to_string),
        persisted_active_permission_level: seed.persisted_active_permission_level,
        work_dir: ctx.work_dir.to_path_buf(),
        workspace_is_set: ctx.workspace_is_set,
        temperature_override: ctx.temperature_override,
        seed_override: ctx.seed_override,
        llm_override: ctx.llm_override.clone(),
        executor_llm_override: ctx.executor_llm_override.clone(),
        readonly_tool_ttl_cache_secs: ctx.readonly_tool_ttl_cache_secs,
        // 没有 HTTP 请求：审计用专门的占位（与 scheduled 区分，见 `auto_resume_placeholder`）。
        request_audit: crate::request_audit::WebRequestAudit::auto_resume_placeholder(),
        // 不能沿用旧回合的 request_id：会与已发出的 SSE 帧混淆。
        request_id: None,
        github_token: ctx.github_token.clone(),
        client_sse_protocol: ctx.client_sse_protocol,
    };

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    queue
        .try_submit_json(JsonSubmitParams { envelope, reply_tx })
        .map_err(|_| "chat queue full".to_string())?;

    // 提交成功后才计轮数：提交失败不该消耗硬闸额度。
    epoch(ctx.app).note_resume(ctx.conversation_id);

    // 没有人在等这个 oneshot：起 task 消费并记日志。续跑结果通过会话历史体现，
    // 不需要回给任何客户端（也就没有 SSE 消费者问题）。
    tokio::spawn(async move {
        match reply_rx.await {
            Ok(Ok(_)) => tracing::info!(
                target: "crabmate::autonomy",
                job_id, "auto-resumed turn finished"
            ),
            Ok(Err(e)) => tracing::warn!(
                target: "crabmate::autonomy",
                job_id, "auto-resumed turn failed: {}", failure_label(&e)
            ),
            Err(_) => tracing::warn!(
                target: "crabmate::autonomy",
                job_id, "auto-resumed turn reply dropped"
            ),
        }
    });
    Ok(job_id)
}

/// 失败原因只取标签，不把整份错误 Debug 打进日志（可能含消息正文）。
fn failure_label(e: &ChatJsonJobFailure) -> &'static str {
    match e {
        ChatJsonJobFailure::ConversationConflict => "conversation_conflict",
        ChatJsonJobFailure::Agent(_) => "agent_error",
    }
}

/// 续跑回合的最小指令（`user.name` 已登记为运行时刷新类：不落盘、不算真实发言）。
fn auto_resume_instruction() -> Message {
    Message {
        role: "user".to_string(),
        content: Some(crate::cm_types::CRABMATE_AUTO_RESUME_TEXT.to_string().into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: Some(crate::cm_types::CRABMATE_AUTO_RESUME_NAME.to_string()),
        tool_call_id: None,
    }
}
