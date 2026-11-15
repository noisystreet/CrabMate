#![cfg(feature = "server")]
//! 编排级集成测：通过 [`crabmate::TurnRequest`] 注入 [`crabmate::llm::ChatCompletionsBackend`]，
//! 钉住 `run_agent_turn` → `run_agent_outer_loop` 的「Planner → 工具 → Planner → 终答」入口链，**不**访问真实网络。
//!
//! **PER 外循环 `OuterLoopDriver`** 路径 mock：
//! - reflect **`BreakOuter`** 单轮停；
//! - 工具后早停 / **`ContinueNextIteration`** 再规划终答；
//! - 冗余探针判定见 **`golden_turn_completion`**（`redundant_tools`）与 **`outer_loop_iteration_reduce`** 金样。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use crabmate::{
    AgentConfig, AgentTurnLlmOverrides, AgentTurnTransport, ChatCompletionsBackend, ChatRequest,
    FunctionCall, LlmSeedOverride, Message, RunAgentTurnAttach, RunAgentTurnObs,
    RunAgentTurnSession, RunAgentTurnSharedInputs, StreamChatParams, ToolCall, TurnProcessHandles,
    TurnRequest, build_tools, load_config, message_content_as_str, run_agent_turn,
};

/// 按序返回预设 assistant 消息；用于编排回归，**非**生产后端。
#[derive(Debug)]
struct SequencedMockBackend {
    responses: Vec<Message>,
    call_seq: AtomicUsize,
    finish_reason: &'static str,
}

impl SequencedMockBackend {
    fn new(responses: Vec<Message>, finish_reason: &'static str) -> Self {
        Self {
            responses,
            call_seq: AtomicUsize::new(0),
            finish_reason,
        }
    }
}

#[async_trait]
impl ChatCompletionsBackend for SequencedMockBackend {
    async fn stream_chat(
        &self,
        _params: &StreamChatParams<'_>,
        req: &mut ChatRequest,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
        let _ = req;
        let idx = self.call_seq.fetch_add(1, Ordering::SeqCst);
        let msg = self
            .responses
            .get(idx)
            .or_else(|| self.responses.last())
            .cloned()
            .ok_or_else(|| "SequencedMockBackend: empty response sequence".to_string())?;
        Ok((msg, self.finish_reason.to_string()))
    }
}

fn cfg_freeform_turn() -> Arc<AgentConfig> {
    let cfg = load_config(None).expect("embedded default config must load");
    Arc::new(cfg)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_outer_loop_tool_round_then_final_assistant() {
    let cfg = cfg_freeform_turn();
    let client = reqwest::Client::new();
    let tools = build_tools();
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only(
            "请分析当前项目的目录结构，并调用 text_transform 工具对字符串 hello 做 sha256 短哈希，然后用一句话总结。".to_string(),
        ),
    ];
    let tool_round = Message {
        role: "assistant".to_string(),
        content: None,
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_mock_1".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "text_transform".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        name: None,
        tool_call_id: None,
    };
    let final_msg = Message::assistant_only(
        "已查询：当前时间可用（mock 编排测），以下为完整终答摘要。".to_string(),
    );
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![tool_round, final_msg.clone()],
        "stop",
    )));

    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let params = TurnRequest {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools: tools.as_slice(),
        },
        session: RunAgentTurnSession {
            messages: &mut messages,
            effective_working_dir: work_dir,
            workspace_is_set: true,
        },
        transport: AgentTurnTransport {
            out: None,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            sse_control_mirror: None,
            llm_backend: Some(backend as &dyn ChatCompletionsBackend),
            trace_sink: None,
        },
        llm: AgentTurnLlmOverrides {
            temperature_override: None,
            model_override: None,
            use_executor_model: false,
            executor_model_override: None,
            executor_api_base: None,
            executor_api_key: None,
            seed_override: LlmSeedOverride::default(),
        },
        attach: RunAgentTurnAttach {
            long_term_memory: None,
            long_term_memory_scope_id: None,
            read_file_turn_cache: None,
            turn_allowed_tool_names: None,
            permission: crabmate::cm_agent::permission::TurnPermissionStages {
                role_max: None,
                session_mode: crabmate::types::SessionMode::Act,
                requested: None,
                persisted: None,
            },
        },
        obs: RunAgentTurnObs {
            tracing_chat_turn: None,
            request_audit: None,
            process_handles: TurnProcessHandles::default_arc(),
            tool_job_registry: None,
        },
    };

    run_agent_turn(params)
        .await
        .expect("mock turn must succeed");
    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        2,
        "expected freeform outer loop: planner(tool) → planner(final)"
    );
    let last = messages.last().expect("at least one message after turn");
    assert_eq!(last.role, "assistant");
    let body = message_content_as_str(&last.content)
        .unwrap_or("")
        .to_string();
    assert!(
        body.contains("mock 编排测"),
        "last assistant should be final mock body, got {body:?}"
    );
}

async fn run_mock_agent_turn<B>(
    cfg: Arc<AgentConfig>,
    messages: &mut Vec<Message>,
    backend: &'static B,
) where
    B: ChatCompletionsBackend + 'static,
{
    let client = reqwest::Client::new();
    let tools = build_tools();
    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let params = TurnRequest {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools: tools.as_slice(),
        },
        session: RunAgentTurnSession {
            messages,
            effective_working_dir: work_dir,
            workspace_is_set: true,
        },
        transport: AgentTurnTransport {
            out: None,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            sse_control_mirror: None,
            llm_backend: Some(backend as &dyn ChatCompletionsBackend),
            trace_sink: None,
        },
        llm: AgentTurnLlmOverrides {
            temperature_override: None,
            model_override: None,
            use_executor_model: false,
            executor_model_override: None,
            executor_api_base: None,
            executor_api_key: None,
            seed_override: LlmSeedOverride::default(),
        },
        attach: RunAgentTurnAttach {
            long_term_memory: None,
            long_term_memory_scope_id: None,
            read_file_turn_cache: None,
            turn_allowed_tool_names: None,
            permission: crabmate::cm_agent::permission::TurnPermissionStages {
                role_max: None,
                session_mode: crabmate::types::SessionMode::Act,
                requested: None,
                persisted: None,
            },
        },
        obs: RunAgentTurnObs {
            tracing_chat_turn: None,
            request_audit: None,
            process_handles: TurnProcessHandles::default_arc(),
            tool_job_registry: None,
        },
    };
    run_agent_turn(params)
        .await
        .expect("mock run_agent_turn must succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_compacts_only_model_view_and_keeps_canonical_history() {
    let mut cfg = (*cfg_freeform_turn()).clone();
    cfg.llm_sampling.llm_context_tokens = 4_096;
    cfg.llm_sampling.max_tokens = 512;
    cfg.context_pipeline.context_token_safety_margin_tokens = 128;
    cfg.context_pipeline.context_token_trigger_percent = 50;
    cfg.context_pipeline.context_token_target_percent = 40;
    let cfg = Arc::new(cfg);
    let old_answer = "old canonical answer";
    let mut messages = vec![
        Message::system_only("test system"),
        Message::user_only("x".repeat(12_000)),
        Message::assistant_only(old_answer),
        Message::user_only("current user"),
    ];
    let final_msg = Message::assistant_only("current final");
    let backend: &'static SequencedMockBackend =
        Box::leak(Box::new(SequencedMockBackend::new(vec![final_msg], "stop")));

    run_mock_agent_turn(cfg, &mut messages, backend).await;

    assert!(
        messages
            .iter()
            .any(|message| { message_content_as_str(&message.content) == Some(old_answer) })
    );
    assert!(
        messages
            .iter()
            .any(crabmate::types::is_model_context_artifact_marker)
    );
    assert_eq!(
        messages
            .last()
            .and_then(|message| message_content_as_str(&message.content)),
        Some("current final")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_plan_rewrite_exhausted_on_missing_plan() {
    use crabmate::agent::per_coord::FinalPlanRequirementMode;

    let mut cfg = (*cfg_freeform_turn()).clone();
    cfg.per_plan_policy.final_plan_requirement = FinalPlanRequirementMode::Always;
    // 运行时 `PerCoordinator` 将 0 钳制为 1；用 1 测「首答缺规划 → 一次重写 → 仍缺规划则耗尽」。
    cfg.per_plan_policy.plan_rewrite_max_attempts = 1;
    let cfg = Arc::new(cfg);
    let client = reqwest::Client::new();
    let tools = build_tools();
    let final_without_plan =
        Message::assistant_only("这是没有 agent_reply_plan 的终答。".to_string());
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![final_without_plan.clone(), final_without_plan.clone()],
        "stop",
    )));

    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only("你好，请用一句话介绍你自己。".to_string()),
    ];
    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let params = TurnRequest {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools: tools.as_slice(),
        },
        session: RunAgentTurnSession {
            messages: &mut messages,
            effective_working_dir: work_dir,
            workspace_is_set: true,
        },
        transport: AgentTurnTransport {
            out: None,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            sse_control_mirror: None,
            llm_backend: Some(backend as &dyn ChatCompletionsBackend),
            trace_sink: None,
        },
        llm: AgentTurnLlmOverrides {
            temperature_override: None,
            model_override: None,
            use_executor_model: false,
            executor_model_override: None,
            executor_api_base: None,
            executor_api_key: None,
            seed_override: LlmSeedOverride::default(),
        },
        attach: RunAgentTurnAttach {
            long_term_memory: None,
            long_term_memory_scope_id: None,
            read_file_turn_cache: None,
            turn_allowed_tool_names: None,
            permission: crabmate::cm_agent::permission::TurnPermissionStages {
                role_max: None,
                session_mode: crabmate::types::SessionMode::Act,
                requested: None,
                persisted: None,
            },
        },
        obs: RunAgentTurnObs {
            tracing_chat_turn: None,
            request_audit: None,
            process_handles: TurnProcessHandles::default_arc(),
            tool_job_registry: None,
        },
    };

    run_agent_turn(params)
        .await
        .expect("turn should stop after plan_rewrite exhausted without hard error");
    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        2,
        "plan_rewrite_max_attempts=1: initial planner + one rewrite before exhausted stop"
    );
    assert!(
        messages.iter().any(|m| m.role == "assistant"),
        "assistant message should remain in history"
    );
}

/// 外循环 reflect → **`BreakOuter`**：首轮终答无 `tool_calls`，仅一次 mock LLM（`OuterLoopDriver` 早停）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_outer_loop_reflect_break_single_planner_mock() {
    let cfg = cfg_freeform_turn();
    let final_body = "outer_loop reflect break mock：这是足够长的终答摘要，满足可见终答阈值。";
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![Message::assistant_only(final_body.to_string())],
        "stop",
    )));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only("请用一句话介绍你自己。".to_string()),
    ];
    run_mock_agent_turn(cfg, &mut messages, backend).await;
    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        1,
        "reflect BreakOuter: single planner call then stop"
    );
}

/// 外循环工具后早停：预置完成证据后单轮终答（`reduce_outer_loop_post_tools_exit` → **`StopOuterLoop`**）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_outer_loop_post_tools_early_stop_mock() {
    let cfg = cfg_freeform_turn();
    let final_msg = Message::assistant_only(
        "outer_loop post-tools early stop mock：任务已有完成证据，终答摘要。".to_string(),
    );
    let backend: &'static SequencedMockBackend =
        Box::leak(Box::new(SequencedMockBackend::new(vec![final_msg], "stop")));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only(
            "请分析当前项目的目录结构，并调用 text_transform 工具对字符串 hello 做 sha256 短哈希，然后用一句话总结。".to_string(),
        ),
        Message {
            role: "tool".to_string(),
            content: Some(
                "当前时间：2026-01-01 00:00:00\n退出码：0\n标准输出：\nok".to_string().into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("text_transform".to_string()),
            tool_call_id: Some("call_mock_0".to_string()),
        },
        Message::assistant_only(
            "目录结构已分析完成；时间已查询，以下为完整终答摘要（mock 早停路径）。".to_string(),
        ),
    ];
    run_mock_agent_turn(cfg, &mut messages, backend).await;
    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        1,
        "post-tools early stop: single planner after pre-seeded completion evidence"
    );
}

/// 外循环工具后 **`ContinueNextIteration`**：两轮工具各一次，第三轮终答停轮（3 次 mock LLM）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_outer_loop_post_tools_continue_then_stop_mock() {
    let cfg = cfg_freeform_turn();
    let tool_round = Message {
        role: "assistant".to_string(),
        content: None,
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_mock_1".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "text_transform".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        name: None,
        tool_call_id: None,
    };
    let final_msg = Message::assistant_only(
        "outer_loop post-tools continue mock：已两次查询时间并完成总结。".to_string(),
    );
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![tool_round.clone(), tool_round, final_msg],
        "stop",
    )));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only(
            "请分析当前项目的目录结构，并调用 text_transform 工具对字符串 hello 做 sha256 短哈希，然后用一句话总结。".to_string(),
        ),
    ];
    run_mock_agent_turn(cfg, &mut messages, backend).await;
    let tool_msg_count = messages.iter().filter(|m| m.role == "tool").count();
    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        3,
        "post-tools continue: tool → tool → final; tool_msgs={tool_msg_count}"
    );
    assert!(
        tool_msg_count >= 1,
        "at least one tool round should execute; roles: {:?}",
        messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>()
    );
}

/// 按「judge / 主循环」分流计数的 mock 后端。
///
/// judge 的 system 以「任务完成度审计员」开头，据此与主循环 planner 调用区分。
#[derive(Debug)]
struct JudgeCountingBackend {
    main_responses: Vec<Message>,
    main_seq: AtomicUsize,
    judge_calls: AtomicUsize,
    main_calls: AtomicUsize,
}

impl JudgeCountingBackend {
    fn new(main_responses: Vec<Message>) -> Self {
        Self {
            main_responses,
            main_seq: AtomicUsize::new(0),
            judge_calls: AtomicUsize::new(0),
            main_calls: AtomicUsize::new(0),
        }
    }

    fn judge_calls(&self) -> usize {
        self.judge_calls.load(Ordering::SeqCst)
    }
}

fn is_judge_request(req: &ChatRequest) -> bool {
    req.messages
        .first()
        .and_then(|m| message_content_as_str(&m.content))
        .is_some_and(|s| s.contains("审计员"))
}

#[async_trait]
impl ChatCompletionsBackend for JudgeCountingBackend {
    async fn stream_chat(
        &self,
        _params: &StreamChatParams<'_>,
        req: &mut ChatRequest,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
        if is_judge_request(req) {
            self.judge_calls.fetch_add(1, Ordering::SeqCst);
            // 恒判 unsatisfied：不触发早停，好让外循环继续跑到预设轮数。
            return Ok((
                Message::assistant_only(r#"{"status":"unsatisfied"}"#),
                "stop".to_string(),
            ));
        }
        self.main_calls.fetch_add(1, Ordering::SeqCst);
        let idx = self.main_seq.fetch_add(1, Ordering::SeqCst);
        let msg = self
            .main_responses
            .get(idx)
            .or_else(|| self.main_responses.last())
            .cloned()
            .ok_or_else(|| "JudgeCountingBackend: empty response sequence".to_string())?;
        let finish = if msg.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
            "tool_calls"
        } else {
            "stop"
        };
        Ok((msg, finish.to_string()))
    }
}

fn readonly_tool_round(id: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: None,
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![ToolCall {
            id: id.to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "text_transform".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        name: None,
        tool_call_id: None,
    }
}

/// **量化 judge 的实际触发次数**（不是「理论上」）。
///
/// 场景：只读分析任务（第 ① 层恒 `NotApplicable`）+ N 轮工具批，judge 恒判 unsatisfied
/// 使轮次跑满。
///
/// 期望行为：**每轮产生新证据才问一次**。因此 N 轮 → N 次。若某轮没有新工具，
/// 指纹不变，不再问。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completion_judge_call_count_tracks_new_evidence() {
    let mut cfg = (*cfg_freeform_turn()).clone();
    cfg.per_plan_policy.completion_judge_enabled = true;
    let cfg = Arc::new(cfg);

    let rounds = 5usize;
    let mut responses: Vec<Message> = (0..rounds)
        .map(|i| readonly_tool_round(&format!("call_judge_{i}")))
        .collect();
    responses.push(Message::assistant_only(
        "分析完成，当前项目包含若干目录，总结如上。".to_string(),
    ));

    let backend: &'static JudgeCountingBackend =
        Box::leak(Box::new(JudgeCountingBackend::new(responses)));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only("分析当前项目的目录结构".to_string()),
    ];
    run_mock_agent_turn(cfg, &mut messages, backend).await;

    let calls = backend.judge_calls();
    eprintln!(
        "PROBE completion_judge_calls={calls} (rounds={rounds}, main_calls={})",
        backend.main_calls.load(Ordering::SeqCst)
    );
    assert!(calls >= 1, "开启后至少应问一次（否则说明接线未生效）");
    // 每回合硬上限 3：即使 5 轮都有新证据（5 个不同指纹），也只允许问 3 次。
    assert_eq!(
        calls, 3,
        "应恰好停在每回合上限 3（rounds={rounds}），got {calls}"
    );
}
