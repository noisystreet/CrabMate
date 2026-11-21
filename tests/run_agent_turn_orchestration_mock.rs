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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

/// 记录每轮请求可见工具名的 mock：用于验证回合执行约束判定确实收窄了工具表。
#[derive(Debug)]
struct ToolCapturingBackend {
    responses: Vec<Message>,
    call_seq: AtomicUsize,
    seen_tool_names: std::sync::Mutex<Vec<Vec<String>>>,
}

impl ToolCapturingBackend {
    fn new(responses: Vec<Message>) -> Self {
        Self {
            responses,
            call_seq: AtomicUsize::new(0),
            seen_tool_names: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn seen_tool_names(&self) -> Vec<Vec<String>> {
        self.seen_tool_names
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl ChatCompletionsBackend for ToolCapturingBackend {
    async fn stream_chat(
        &self,
        _params: &StreamChatParams<'_>,
        req: &mut ChatRequest,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
        let names = req
            .tools
            .as_ref()
            .map(|ts| ts.iter().map(|t| t.function.name.clone()).collect())
            .unwrap_or_default();
        if let Ok(mut seen) = self.seen_tool_names.lock() {
            seen.push(names);
        }
        let idx = self.call_seq.fetch_add(1, Ordering::SeqCst);
        let msg = self
            .responses
            .get(idx)
            .or_else(|| self.responses.last())
            .cloned()
            .ok_or_else(|| "ToolCapturingBackend: empty response sequence".to_string())?;
        Ok((msg, "stop".to_string()))
    }
}

fn cfg_freeform_turn() -> Arc<AgentConfig> {
    let mut cfg = load_config(None).expect("embedded default config must load");
    // 本文件用一个**共享的 sequenced backend** 同时服务执行路径与各语义判定，而 `call_seq`
    // 无法区分它们——任何一个默认开启的判定层都会给回合额外加调用、直接打乱预设响应序列。
    // 故这里显式关掉两者：本文件测的是外层编排，不是判定。
    // 判定本身的覆盖见 `completion_judge_call_count_tracks_new_evidence`（显式开启）与
    // `at_turn_start::tests`（回合约束判定的解析与 fail-open）。
    cfg.per_plan_policy.completion_judge_enabled = false;
    cfg.per_plan_policy.turn_execution_constraint_judge_enabled = false;
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
            paused_reason_sink: None,
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
            paused_reason_sink: None,
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
            paused_reason_sink: None,
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

/// #1414：早停门放行、但本轮 assistant 只带 `tool_calls`（无正文）时**不能**直接停轮。
///
/// 期望：注入终答缺失纠偏 → `ContinueNextIteration` → 第二次模型调用产出正文后才停。
/// 修复前这条路径只调用一次模型就 `stop_outer_loop`，回合没有任何终答。
///
/// #1415 删掉第 ① 层后，早停只能由 judge 放行，故这里用 judge 返 `satisfied` 触发，
/// 而不是预置结构化证据（C++ 三件套那条路径已不存在）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_post_tools_early_stop_without_body_asks_for_final_answer_mock() {
    let mut cfg = (*cfg_freeform_turn()).clone();
    cfg.per_plan_policy.completion_judge_enabled = true;
    let cfg = Arc::new(cfg);
    // 关键：正文为空，只有 `tool_calls`。
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
        "已跑通全部测试（12 passed），无回归，以下为完整终答摘要。".to_string(),
    );
    let backend: &'static JudgeCountingBackend =
        Box::leak(Box::new(JudgeCountingBackend::new_with_verdict(
            vec![tool_round, final_msg],
            r#"{"status":"satisfied","evidence_refs":["ev_1"]}"#,
        )));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only("跑通全部测试，然后回报结果".to_string()),
        // 锚点校验要求 `satisfied` 至少引用一条 verify / write 成功证据。
        Message {
            role: "tool".to_string(),
            content: Some("已修改文件: src/x.rs".to_string().into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("modify_file".to_string()),
            tool_call_id: Some("call_seed".to_string()),
        },
    ];
    run_mock_agent_turn(cfg, &mut messages, backend).await;

    assert_eq!(
        backend.main_calls.load(Ordering::SeqCst),
        2,
        "早停门放行但无终答时应再要一轮而不是停轮：roles={:?}",
        messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>()
    );
    assert!(backend.judge_calls() >= 1, "早停门必须真的问过判定层");
    let last_assistant = messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .expect("回合应留下 assistant 消息");
    let body = message_content_as_str(&last_assistant.content).unwrap_or_default();
    assert!(
        body.trim().chars().count() >= 24,
        "末条 assistant 必须是可见终答，实际: {body:?}"
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
    /// judge 固定回的 JSON（默认 `unsatisfied`，使外循环跑满预设轮数）。
    judge_verdict: &'static str,
}

const JUDGE_UNSATISFIED: &str = r#"{"status":"unsatisfied"}"#;

impl JudgeCountingBackend {
    fn new(main_responses: Vec<Message>) -> Self {
        Self::new_with_verdict(main_responses, JUDGE_UNSATISFIED)
    }

    fn new_with_verdict(main_responses: Vec<Message>, judge_verdict: &'static str) -> Self {
        Self {
            main_responses,
            main_seq: AtomicUsize::new(0),
            judge_calls: AtomicUsize::new(0),
            main_calls: AtomicUsize::new(0),
            judge_verdict,
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
            return Ok((
                Message::assistant_only(self.judge_verdict),
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

// ---------------------------------------------------------------------------
// #839 P1 金样：子任务超预算被掐断，父回合仍正常结束。
// ---------------------------------------------------------------------------

/// 按「父 / 子」分流：子回合（system 含「只读探索子任务」）恒返回 tool_calls 以撞 LLM 次数上限；
/// 父回合按序返回预设响应。
struct ParentChildAwareBackend {
    parent_responses: Vec<Message>,
    parent_seq: AtomicUsize,
    child_calls: AtomicUsize,
    /// 父回合是否在**后续请求**里看到了 `tool` 消息（即子任务的 report）。
    ///
    /// 断言这个而不是最终 `messages`：回合收尾会重写 canonical history，
    /// 而「父回合实际看到什么」以送进模型的请求为准。
    parent_saw_tool_result: AtomicBool,
}

impl ParentChildAwareBackend {
    fn new(parent_responses: Vec<Message>) -> Self {
        Self {
            parent_responses,
            parent_seq: AtomicUsize::new(0),
            child_calls: AtomicUsize::new(0),
            parent_saw_tool_result: AtomicBool::new(false),
        }
    }
}

fn is_child_request(req: &ChatRequest) -> bool {
    req.messages
        .first()
        .and_then(|m| message_content_as_str(&m.content))
        .is_some_and(|s| s.contains("只读探索子任务"))
}

#[async_trait]
impl ChatCompletionsBackend for ParentChildAwareBackend {
    async fn stream_chat(
        &self,
        _params: &StreamChatParams<'_>,
        req: &mut ChatRequest,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
        if is_child_request(req) {
            self.child_calls.fetch_add(1, Ordering::SeqCst);
            // 子回合永远「继续探索」→ 父配额（`subagent_max_llm_calls = 1`）被撞满。
            let tc = Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "child_call".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "list_tree".to_string(),
                        arguments: "{\"path\": \".\"}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            };
            return Ok((tc, "tool_calls".to_string()));
        }
        if req.messages.iter().any(|m| m.role == "tool") {
            self.parent_saw_tool_result.store(true, Ordering::SeqCst);
        }
        let idx = self.parent_seq.fetch_add(1, Ordering::SeqCst);
        let msg = self
            .parent_responses
            .get(idx)
            .or_else(|| self.parent_responses.last())
            .cloned()
            .ok_or_else(|| "ParentChildAwareBackend: empty responses".to_string())?;
        let finish = if msg.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
            "tool_calls"
        } else {
            "stop"
        };
        Ok((msg, finish.to_string()))
    }
}

/// **金样**：子任务超预算被掐断，父回合仍能正常结束（#839「第一刀」验收）。
///
/// 与单测 `budget_exhausted_does_not_propagate_to_parent` 的区别：那条只测 `delegator.run()`
/// 返回 `Ok(report)`；这条走**真实的父回合编排**——模型看到 `delegate_task` → 分发 →
/// 子回合真的跑起来并撞 LLM 次数上限 → 父回合收到 report 后**继续**跑出终答。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagent_budget_exhausted_parent_turn_still_finishes() {
    let mut cfg = (*cfg_freeform_turn()).clone();
    cfg.subagent.enabled = true;
    // 子回合只允许 1 次 LLM 调用 → 它第 2 次就会被掐断。
    cfg.subagent.max_llm_calls = 1;
    cfg.subagent.max_wall_secs = 300;
    let cfg = Arc::new(cfg);

    let delegate_round = Message {
        role: "assistant".to_string(),
        content: None,
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![ToolCall {
            id: "parent_delegate".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "delegate_task".to_string(),
                arguments: "{\"prompt\":\"看看 src 下有什么\"}".to_string(),
            },
        }]),
        name: None,
        tool_call_id: None,
    };
    let final_msg =
        Message::assistant_only("子任务已返回（可能被掐断），本轮结论如下：项目结构可读。");

    let backend: &'static ParentChildAwareBackend =
        Box::leak(Box::new(ParentChildAwareBackend::new(vec![
            delegate_round,
            final_msg,
        ])));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only("看看这个项目的目录结构".to_string()),
    ];
    run_mock_agent_turn(cfg, &mut messages, backend).await;

    // 1) 子回合确实跑过（否则本金样什么都没验到）。
    assert!(
        backend.child_calls.load(Ordering::SeqCst) >= 1,
        "子任务应真的跑起来（否则金样没驗到东西）"
    );

    // 2) 父回合**正常结束**：最后一条是 assistant 正文，不是错误。
    let last = messages.last().expect("父回合应产出终答");
    assert_eq!(last.role, "assistant", "最后一条应是 assistant 终答");
    let body = message_content_as_str(&last.content).unwrap_or("");
    assert!(
        body.contains("本轮结论如下"),
        "父回合应跑出终答，实际正文：{body}"
    );

    // 3) 父回合**实际看到**了子任务的 report（以送进模型的请求为准）。
    assert!(
        backend.parent_saw_tool_result.load(Ordering::SeqCst),
        "父回合后续请求里应出现 tool 消息（子任务的 report）"
    );
}

/// 回合起点执行约束判定（#1401 #12）：判定为只读时，planner 请求的工具表必须被收窄。
///
/// 这条路径此前**零覆盖**——`cfg_freeform_turn` 关掉了开关，单测只测 `parse_judge_reply`。
/// 而它正是替代关键词启发式的落点：判定为 `true` 后 `step_executor_constraint` 是否真的生效，
/// 只能靠「planner 拿到的工具表里没有写工具」来验证。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_constraint_judge_narrows_planner_tools_when_readonly() {
    let mut cfg = load_config(None).expect("embedded default config must load");
    cfg.per_plan_policy.completion_judge_enabled = false;
    cfg.per_plan_policy.turn_execution_constraint_judge_enabled = true;
    // 第 0 次 = 回合起点判定；第 1 次 = planner 终答。
    let backend: &'static ToolCapturingBackend =
        Box::leak(Box::new(ToolCapturingBackend::new(vec![
            Message::assistant_only(r#"{"review_readonly":true}"#.to_string()),
            Message::assistant_only("这是一段足够长的终答摘要，用于结束本轮。".to_string()),
        ])));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only("别动代码，先分析一下这个报错。".to_string()),
    ];
    run_mock_agent_turn(Arc::new(cfg), &mut messages, backend).await;

    let seen = backend.seen_tool_names();
    assert_eq!(
        seen.len(),
        2,
        "应有判定期 + planner 期各一次请求，实际 {seen:?}"
    );
    assert!(
        seen[0].is_empty(),
        "判定请求不应携带 tools，实际 {:?}",
        seen[0]
    );
    let planner_tools = &seen[1];
    assert!(!planner_tools.is_empty(), "planner 工具表不应为空");
    for write_tool in [
        "apply_patch",
        "create_file",
        "modify_file",
        "append_file",
        "search_replace",
    ] {
        assert!(
            !planner_tools.iter().any(|n| n == write_tool),
            "ReviewReadonly 下不应暴露写工具 {write_tool}：{planner_tools:?}"
        );
    }
}
