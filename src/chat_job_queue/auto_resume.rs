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
use crate::cm_agent::run_epoch::{EpochSnapshot, RunEpoch};
use crate::cm_agent::task_state::{
    AutoResumeDecision, AutoResumeStopReason, TaskPauseReason, decide_auto_resume,
};
use crate::cm_config::AgentConfig;
use crate::cm_types::Message;
use crate::context::web_state_facet::WebChatJobAppFacet;
use crate::types::LlmSeedOverride;

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
    /// 本回合是否被用户取消（取消优先于一切，含开关）。
    pub(super) cancelled: bool,
    /// 本回合是否**由自动续跑发起**——决定暂停是否计入「连续同因」。
    pub(super) from_auto_resume: bool,
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

/// 自动续跑的**决策结果**（纯函数产出，不含任何 IO）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AutoResumePlan {
    /// 应续跑，使用这个（写回后的）revision。
    Resume { new_rev: u64 },
    /// 不续跑。
    Skip(AutoResumeSkip),
}

/// 不续跑的原因。前三个是**前置条件**，最后一个来自 [`decide_auto_resume`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AutoResumeSkip {
    /// 本回合被用户取消——**取消优先级最高**，任何其它条件都不该让它续跑。
    Cancelled,
    /// `enabled` 或 `auto_resume` 关闭（默认）。
    SwitchedOff,
    /// 本回合不是因预算暂停结束。
    NotPaused,
    /// 会话未成功落盘 ⇒ 没有可靠 revision，**不得**退化成 `None`（那会放弃 CAS）。
    NotSaved,
    /// 交给 [`decide_auto_resume`] 判定的闸门（审批 / 轮数 / 无进展）。
    Budget(AutoResumeStopReason),
}

impl AutoResumeSkip {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::SwitchedOff => "switched_off",
            Self::NotPaused => "not_paused",
            Self::NotSaved => "not_saved",
            Self::Budget(r) => r.as_str(),
        }
    }
}

/// 判定「是否应续跑、用哪个 revision」。
///
/// **纯函数**：不读时钟、不碰锁、不做 IO——这样每条分支都能被单测钉住，
/// 也让 `maybe_auto_resume_next_turn` 只剩接线。
///
/// 顺序与 [`decide_auto_resume`] 一致地「安全优先」：开关 → 是否真暂停 → **是否已落盘**
/// → 各闸门。`NotSaved` 排在闸门之前，因为拿不到 revision 时后面判什么都无意义。
pub(super) fn plan_auto_resume(
    autonomy: &AgentConfig,
    paused_reason: Option<TaskPauseReason>,
    saved_revision: Option<u64>,
    cancelled: bool,
    snap: EpochSnapshot,
) -> AutoResumePlan {
    // 取消优先于一切：用户刚点了停止，绝不能替他再起一回合。
    if cancelled {
        return AutoResumePlan::Skip(AutoResumeSkip::Cancelled);
    }
    let a = &autonomy.autonomy;
    // 生产路径上这条**走不到**：`maybe_auto_resume_next_turn` 在拿锁之前就已短路（零 IO）。
    // 保留它是因为 plan 是独立纯函数——将来若有调用方绕过那个短路，这里是最后一道闸。
    // 对应测试 `switched_off_wins_over_everything` 测的是这条防御分支，不是热路径。
    if !a.enabled || !a.auto_resume {
        return AutoResumePlan::Skip(AutoResumeSkip::SwitchedOff);
    }
    let Some(reason) = paused_reason else {
        return AutoResumePlan::Skip(AutoResumeSkip::NotPaused);
    };
    let Some(new_rev) = saved_revision else {
        return AutoResumePlan::Skip(AutoResumeSkip::NotSaved);
    };
    match decide_auto_resume(
        a,
        Some(reason),
        snap.resumed_turns,
        snap.consecutive_same_reason,
        false,
    ) {
        AutoResumeDecision::Go => AutoResumePlan::Resume { new_rev },
        AutoResumeDecision::Stop(why) => AutoResumePlan::Skip(AutoResumeSkip::Budget(why)),
    }
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

    // 先记账（`record_pause` 会更新「连续同因」），再取快照交给纯函数判定。
    let snap = {
        let mut e = epoch(ctx.app);
        e.record_pause(ctx.conversation_id, reason, ctx.from_auto_resume);
        e.snapshot(ctx.conversation_id)
    };

    let new_rev = match plan_auto_resume(
        ctx.cfg_snap,
        Some(reason),
        ctx.saved_revision,
        ctx.cancelled,
        snap,
    ) {
        AutoResumePlan::Resume { new_rev } => new_rev,
        AutoResumePlan::Skip(why) => {
            tracing::info!(
                target: "crabmate::autonomy",
                conversation_id = %ctx.conversation_id,
                reason = reason.as_str(),
                skip = why.as_str(),
                "auto-resume skipped"
            );
            return;
        }
    };

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
        content: Some(
            crate::cm_types::CRABMATE_AUTO_RESUME_TEXT
                .to_string()
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: Some(crate::cm_types::CRABMATE_AUTO_RESUME_NAME.to_string()),
        tool_call_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(auto_resume: bool, max_turns: u32) -> AgentConfig {
        let mut c = crate::config::load_config(None).expect("default config");
        c.autonomy.enabled = true;
        c.autonomy.auto_resume = auto_resume;
        c.autonomy.max_autonomous_turns = max_turns;
        // 默认 true；本组测试关心「默认会不会因为审批而停」。
        c.autonomy.stop_on_approval_required = true;
        c
    }

    fn snap(resumed: u32, consecutive: u32) -> EpochSnapshot {
        EpochSnapshot {
            resumed_turns: resumed,
            consecutive_same_reason: consecutive,
            last_pause_reason: None,
        }
    }

    /// 真 e2e（函数级）：决策 → 入队 → **队列真的执行了那一轮**。
    ///
    /// 用可控 `TurnEngine` 代替真 LLM：它既计数，又钉住「入队的消息里确实带续跑指令」。
    /// 这比「`try_submit_json` 返回 Ok」强——那只证明入队，不证明它会跑。
    #[tokio::test]
    async fn go_actually_enqueues_and_runs_a_turn() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingEngine {
            calls: Arc<AtomicUsize>,
            saw_instruction: Arc<AtomicUsize>,
        }

        #[async_trait::async_trait]
        impl crate::turn_runner::TurnEngine for CountingEngine {
            async fn run(
                &self,
                params: crate::TurnRequest<'_>,
            ) -> Result<crate::turn_runner::TurnOutcome, crate::agent::agent_turn::RunAgentTurnError>
            {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if params
                    .session
                    .messages
                    .iter()
                    .any(crate::cm_types::is_auto_resume_injection)
                {
                    self.saw_instruction.fetch_add(1, Ordering::SeqCst);
                }
                Ok(crate::turn_runner::TurnOutcome {
                    commit: crate::agent::model_context_view::StructuralCommit::default(),
                    stats: crate::turn_runner::TurnStats::default(),
                })
            }
        }

        let mut fixed = crate::config::load_config(None).expect("cfg");
        fixed.autonomy.enabled = true;
        fixed.autonomy.auto_resume = true;
        // 两种 `cfg` 形状：`cfg_snap` 要 `Arc<AgentConfig>`，队列/会话要 `SharedAgentConfig`。
        let agent_cfg: Arc<AgentConfig> = Arc::new(fixed);
        let shared_cfg: crate::cm_config::SharedAgentConfig =
            Arc::new(tokio::sync::RwLock::new((*agent_cfg).clone()));

        let calls = Arc::new(AtomicUsize::new(0));
        let saw = Arc::new(AtomicUsize::new(0));
        let chat_queue = crate::chat_job_queue::ChatJobQueue::new(1, 8);
        let queue_deps = Arc::new(crate::chat_job_queue::WebChatQueueDeps {
            cfg: Arc::clone(&shared_cfg),
            api_key: String::new(),
            client: reqwest::Client::new(),
            tools: crate::build_tools(),
            chat_queue,
            long_term_memory: None,
            sse_stream_hub: Arc::new(crate::chat_job_queue::SseStreamHub::new()),
            llm_backend: None,
            turn_runner: Arc::new(CountingEngine {
                calls: Arc::clone(&calls),
                saw_instruction: Arc::clone(&saw),
            }),
        });

        let app = crate::context::web_state_facet::WebChatJobAppFacet {
            conversation: crate::context::web_state_facet::AppStateConversationRuntime {
                conversation_backing: Arc::new(tokio::sync::RwLock::new(
                    crate::cm_session_store::ConversationBacking::memory_default(),
                )),
                conversation_id_counter: Arc::new(std::sync::atomic::AtomicU64::new(1)),
                cfg: Arc::clone(&shared_cfg),
            },
            run_epoch: Arc::new(std::sync::Mutex::new(RunEpoch::new())),
            process_handles: crate::process_handles::TurnProcessHandles::default_arc(),
            approval_sessions: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            tool_job_registry: crate::jobs::tool_jobs::registry_from_config(&agent_cfg),
        };

        // 先造一条会话：续跑会 `load_conversation_seed` 读它。
        let saved = app
            .conversation
            .save_conversation_messages_if_revision(
                "conv-e2e".to_string(),
                vec![crate::cm_types::Message::user_only("跑个任务")],
                None,
                None,
                None,
                None,
                Vec::new(),
            )
            .await;
        assert_eq!(
            saved,
            crate::conversation_store::SaveConversationOutcome::Saved
        );
        let rev = app
            .conversation
            .load_conversation_seed("conv-e2e")
            .await
            .and_then(|s| s.expected_revision)
            .expect("seed revision");

        maybe_auto_resume_next_turn(AutoResumeCtx {
            queue_deps: &queue_deps,
            app: &app,
            cfg_snap: &agent_cfg,
            conversation_id: "conv-e2e",
            paused_reason: Some(TaskPauseReason::BudgetTokens),
            saved_revision: Some(rev),
            cancelled: false,
            from_auto_resume: false,
            request_agent_role: None,
            request_session_mode: None,
            request_permission_level: None,
            work_dir: std::path::Path::new("."),
            workspace_is_set: true,
            temperature_override: None,
            seed_override: crate::types::LlmSeedOverride::default(),
            llm_override: None,
            executor_llm_override: None,
            readonly_tool_ttl_cache_secs: None,
            github_token: None,
            client_sse_protocol: None,
        })
        .await;

        // 队列是异步的：等那一轮真的被执行。
        for _ in 0..100 {
            if calls.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "续跑回合应被队列执行一次——不只是入队"
        );
        assert_eq!(
            saw.load(Ordering::SeqCst),
            1,
            "入队的消息里必须带续跑指令（magic name）"
        );
        assert_eq!(
            app.run_epoch
                .lock()
                .expect("epoch")
                .snapshot("conv-e2e")
                .resumed_turns,
            1,
            "提交成功后应记一轮"
        );
    }

    /// **取消优先于一切**（含开关）：用户刚点了停止，绝不能替他再起一回合。
    #[test]
    fn cancelled_wins_even_over_switched_on() {
        let c = cfg_with(true, 0);
        assert_eq!(
            plan_auto_resume(
                &c,
                Some(TaskPauseReason::BudgetTokens),
                Some(5),
                true,
                snap(0, 0)
            ),
            AutoResumePlan::Skip(AutoResumeSkip::Cancelled)
        );
    }

    /// 开关关闭时任何其它条件都不该让它续跑。
    ///
    /// 注意：这是 `plan_auto_resume` 的**防御分支**——生产中 `maybe_auto_resume_next_turn`
    /// 在拿锁前就短路了（零 IO），所以本测试保护的是「有人绕过短路」时的兜底。
    #[test]
    fn switched_off_wins_over_everything() {
        let c = cfg_with(false, 0);
        assert_eq!(
            plan_auto_resume(
                &c,
                Some(TaskPauseReason::BudgetTokens),
                Some(7),
                false,
                snap(0, 0)
            ),
            AutoResumePlan::Skip(AutoResumeSkip::SwitchedOff)
        );
    }

    /// 「没暂停」与「没落盘」是**两个不同**的跳过原因——后者是 CAS 安全边界。
    #[test]
    fn not_paused_and_not_saved_are_distinct() {
        let c = cfg_with(true, 0);
        assert_eq!(
            plan_auto_resume(&c, None, Some(7), false, snap(0, 0)),
            AutoResumePlan::Skip(AutoResumeSkip::NotPaused)
        );
        // 关键：暂停了但没有写回后的 revision ⇒ 不得退化成 `None` 入队（那会放弃 CAS）。
        assert_eq!(
            plan_auto_resume(
                &c,
                Some(TaskPauseReason::BudgetLlmCalls),
                None,
                false,
                snap(0, 0)
            ),
            AutoResumePlan::Skip(AutoResumeSkip::NotSaved)
        );
    }

    /// 正常路径：沿用**写回后**的 revision。
    #[test]
    fn resumes_with_the_saved_revision() {
        let c = cfg_with(true, 0);
        assert_eq!(
            plan_auto_resume(
                &c,
                Some(TaskPauseReason::BudgetTokens),
                Some(42),
                false,
                snap(0, 0)
            ),
            AutoResumePlan::Resume { new_rev: 42 }
        );
    }

    /// 硬闸与无进展止损都能拦住「本来可以续」的情形。
    #[test]
    fn gates_skip_before_resume() {
        let capped = cfg_with(true, 2);
        assert_eq!(
            plan_auto_resume(
                &capped,
                Some(TaskPauseReason::BudgetTokens),
                Some(9),
                false,
                snap(2, 0)
            ),
            AutoResumePlan::Skip(AutoResumeSkip::Budget(
                AutoResumeStopReason::MaxTurnsReached
            ))
        );

        let uncapped = cfg_with(true, 0);
        assert_eq!(
            plan_auto_resume(
                &uncapped,
                Some(TaskPauseReason::BudgetTokens),
                Some(9),
                false,
                snap(0, 2)
            ),
            AutoResumePlan::Skip(AutoResumeSkip::Budget(AutoResumeStopReason::NoProgress))
        );
    }

    /// 审批暂停默认不续（等人是安全边界）。
    #[test]
    fn awaiting_approval_stops_by_default() {
        let c = cfg_with(true, 0);
        assert_eq!(
            plan_auto_resume(
                &c,
                Some(TaskPauseReason::AwaitingApproval),
                Some(3),
                false,
                snap(0, 0)
            ),
            AutoResumePlan::Skip(AutoResumeSkip::Budget(
                AutoResumeStopReason::AwaitingApproval
            ))
        );
    }
}
