//! 编排级集成测：通过 [`crabmate::RunAgentTurnParams`] 注入 [`crabmate::llm::ChatCompletionsBackend`]，
//! 钉住 `run_agent_turn` → `run_agent_outer_loop` 的「Planner → 工具 → Planner → 终答」入口链，**不**访问真实网络。
//!
//! 另含分层：
//! - [`crabmate::run_agent_turn`] + **`PlannerExecutorMode::Hierarchical`**：经 `run_hierarchical_agent` →
//!   `runner::run_hierarchical`，与生产入口一致；
//! - **话语型回落**：用户输入命中 **`hierarchical_intent_route::DiscourseFallbackOuter`** 时转
//!   **`run_agent_outer_loop`**，与 **PER / `PerCoordinator`** 轨交汇（见 **`docs/规划执行验证架构.md`** §2.5.2）；
//!
//! **PER 外循环 `OuterLoopDriver`** 路径 mock：
//! - reflect **`BreakOuter`** 单轮停；
//! - 工具后早停 / **`ContinueNextIteration`** 再规划终答；
//! - 冗余探针判定见 **`golden_turn_completion`**（`redundant_tools`）与 **`outer_loop_iteration_reduce`** 金样。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use crabmate::agent::hierarchy::runner::run_hierarchical;
use crabmate::agent::hierarchy::{
    AgentMode, HierarchyRunnerParams, HierarchyRunnerResult, TaskStatus,
};
use crabmate::{
    AgentConfig, AgentTurnLlmOverrides, AgentTurnTransport, ChatCompletionsBackend, ChatRequest,
    FunctionCall, LlmSeedOverride, Message, PlannerExecutorMode, ProcessHandles,
    RunAgentTurnParams, RunAgentTurnSharedInputs, StreamChatParams, ToolCall, build_tools,
    load_config, message_content_as_str, run_agent_turn, shared_static_chat_backend,
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
    let mut cfg = load_config(None).expect("embedded default config must load");
    cfg.per_plan_policy.planner_executor_mode = PlannerExecutorMode::SingleAgent;
    cfg.intent_routing.intent_at_turn_start_enabled = false;
    cfg.intent_routing.intent_l2_enabled = false;
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
            "请分析当前项目的目录结构，并调用 get_current_time 工具查询当前时间，然后用一句话总结。".to_string(),
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
                name: "get_current_time".to_string(),
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
    let params = RunAgentTurnParams {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools: tools.as_slice(),
        },
        messages: &mut messages,
        effective_working_dir: work_dir,
        workspace_is_set: true,
        transport: AgentTurnTransport {
            out: None,
            render_to_terminal: false,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            tui_llm_stream_scratch: None,
            tool_running_hook: None,
            clarification_questionnaire_hook: None,
            sse_control_mirror: None,
            llm_backend: Some(backend as &dyn ChatCompletionsBackend),
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
        long_term_memory: None,
        long_term_memory_scope_id: None,
        read_file_turn_cache: None,
        turn_allowed_tool_names: None,
        tracing_chat_turn: None,
        request_audit: None,
        process_handles: ProcessHandles::default_arc_process_handles(),
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

fn cfg_hierarchical_for_mock_runner() -> Arc<AgentConfig> {
    let mut cfg = load_config(None).expect("embedded default config must load");
    cfg.per_plan_policy.planner_executor_mode = PlannerExecutorMode::Hierarchical;
    cfg.intent_routing.intent_at_turn_start_enabled = false;
    cfg.intent_routing.intent_l2_enabled = false;
    cfg.hierarchy_routing.enable_llm_routing = Some(true);
    Arc::new(cfg)
}

/// 分层 runner：`route_with_llm` → `Manager::decompose_with_llm` → Operator 首轮 `call_llm`（顺序单子目标）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_hierarchical_router_manager_operator_mock_llm_sequence() {
    let cfg = cfg_hierarchical_for_mock_runner();
    let client = Arc::new(reqwest::Client::new());
    let tools = build_tools();
    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let router_json = r#"{"mode":"hierarchical","reasoning":"mock","estimated_steps":3}"#;
    let manager_json = r#"{
  "sub_goals": [
    {
      "goal_id": "goal_1",
      "description": "性能相关：用 get_current_time 查看时间并一句话总结。",
      "priority": 0,
      "depends_on": [],
      "required_tools": ["get_current_time"],
      "goal_type": "analyze"
    }
  ],
  "execution_strategy": "sequential"
}"#;
    let operator_done = Message::assistant_only("子目标已完成 done".to_string());

    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![
            Message::assistant_only(router_json.to_string()),
            Message::assistant_only(manager_json.to_string()),
            operator_done,
        ],
        "stop",
    )));

    let task = "性能相关：请调用 get_current_time 工具查询当前时间，然后用一句话总结。";
    let params = HierarchyRunnerParams {
        task,
        cfg: cfg.as_ref(),
        llm_backend: shared_static_chat_backend(backend),
        client: client.clone(),
        api_key: String::new(),
        working_dir: work_dir.to_path_buf(),
        sse_out: None,
        tools_defs: tools.as_slice(),
        tool_approval_out: None,
        tool_approval_rx: None,
        cancel: None,
        primary_intent: Some("execute.read_inspect".to_string()),
        secondary_intents: Vec::new(),
        intent_mode_bias_enabled: false,
        process_handles: ProcessHandles::default_arc_process_handles(),
        sse_control_mirror: None,
        turn_budget: crabmate::agent::turn_budget::TurnBudgetCounter::new_shared(),
    };

    let outcome: HierarchyRunnerResult = run_hierarchical(params)
        .await
        .expect("hierarchical mock run must succeed");
    assert_eq!(outcome.mode, AgentMode::Hierarchical);
    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        3,
        "expected router LLM → manager decompose LLM → operator call_llm"
    );
    assert!(
        outcome
            .execution_result
            .results
            .iter()
            .any(|r| matches!(r.status, TaskStatus::Completed)),
        "expected at least one completed subgoal, got {:?}",
        outcome.execution_result.results
    );
}

/// `run_agent_turn` → `run_agent_turn_common` → `dispatch_hierarchical_turn` → `run_hierarchical`：
/// 与生产 Web/CLI 相同的 crate 根入口，钉住注入的 `llm_backend` 经 `RunLoopCtx` 传入分层 runner。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_hierarchical_end_to_end_mock_llm_sequence() {
    let cfg = cfg_hierarchical_for_mock_runner();
    let client = reqwest::Client::new();
    let tools = build_tools();

    let router_json = r#"{"mode":"hierarchical","reasoning":"mock","estimated_steps":3}"#;
    let manager_json = r#"{
  "sub_goals": [
    {
      "goal_id": "goal_1",
      "description": "根据用户指令：定位报错原因，必要时只读查看相关文件。",
      "priority": 0,
      "depends_on": [],
      "required_tools": ["get_current_time"],
      "goal_type": "analyze"
    }
  ],
  "execution_strategy": "sequential"
}"#;
    let operator_done = Message::assistant_only("子目标已完成 done".to_string());

    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![
            Message::assistant_only(router_json.to_string()),
            Message::assistant_only(manager_json.to_string()),
            operator_done,
        ],
        "stop",
    )));

    // 须为 L1 **Execute**（避免 `qa.readonly` + `DirectReply` 走话语型回落 `run_agent_outer_loop`，仅消耗一次 mock）。
    let task = "这个报错帮我定位下原因：error[E0425]: cannot find value `x` in this scope";
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only(task.to_string()),
    ];

    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let params = RunAgentTurnParams {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools: tools.as_slice(),
        },
        messages: &mut messages,
        effective_working_dir: work_dir,
        workspace_is_set: true,
        transport: AgentTurnTransport {
            out: None,
            render_to_terminal: false,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            tui_llm_stream_scratch: None,
            tool_running_hook: None,
            clarification_questionnaire_hook: None,
            sse_control_mirror: None,
            llm_backend: Some(backend as &dyn ChatCompletionsBackend),
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
        long_term_memory: None,
        long_term_memory_scope_id: None,
        read_file_turn_cache: None,
        turn_allowed_tool_names: None,
        tracing_chat_turn: None,
        request_audit: None,
        process_handles: ProcessHandles::default_arc_process_handles(),
    };

    run_agent_turn(params)
        .await
        .expect("hierarchical run_agent_turn mock must succeed");

    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        3,
        "expected hierarchical path: router LLM → manager decompose LLM → operator call_llm"
    );

    let last = messages.last().expect("at least one message after turn");
    assert_eq!(last.role, "assistant");
    let body = message_content_as_str(&last.content)
        .unwrap_or("")
        .to_string();
    assert!(
        body.contains("分层执行概览") && body.contains("goal_1"),
        "expected hierarchical finalize summary in last assistant, got len={} preview={:?}",
        body.len(),
        body.chars().take(200).collect::<String>()
    );
}

/// `PlannerExecutorMode::Hierarchical` + 问候类用户句：意图门控 **`ProceedExecute`** 后经
/// `resolve_hierarchical_post_intent_route` → **`DiscourseFallbackOuter`**，进入 **`run_agent_outer_loop`**，
/// 与分层主路径（Router→Manager→Operator）**不**共用 mock 调用序列。钉住 **PER 轨** 与交汇行为。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_hierarchical_discourse_fallback_uses_per_outer_loop() {
    let cfg = cfg_hierarchical_for_mock_runner();
    let client = reqwest::Client::new();
    let tools = build_tools();

    let final_body = "outer_loop mock：话语型回落单轮终答";
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![Message::assistant_only(final_body.to_string())],
        "stop",
    )));

    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only("你好".to_string()),
    ];

    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let params = RunAgentTurnParams {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools: tools.as_slice(),
        },
        messages: &mut messages,
        effective_working_dir: work_dir,
        workspace_is_set: true,
        transport: AgentTurnTransport {
            out: None,
            render_to_terminal: false,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            tui_llm_stream_scratch: None,
            tool_running_hook: None,
            clarification_questionnaire_hook: None,
            sse_control_mirror: None,
            llm_backend: Some(backend as &dyn ChatCompletionsBackend),
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
        long_term_memory: None,
        long_term_memory_scope_id: None,
        read_file_turn_cache: None,
        turn_allowed_tool_names: None,
        tracing_chat_turn: None,
        request_audit: None,
        process_handles: ProcessHandles::default_arc_process_handles(),
    };

    run_agent_turn(params)
        .await
        .expect("hierarchical discourse fallback mock turn must succeed");

    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        1,
        "discourse fallback should use a single outer_loop planner LLM call (no router/manager/operator)"
    );

    let last = messages.last().expect("at least one message after turn");
    assert_eq!(last.role, "assistant");
    let body = message_content_as_str(&last.content)
        .unwrap_or("")
        .to_string();
    assert!(
        body.contains(final_body),
        "expected PER-track final assistant from mock, got preview={:?}",
        body.chars().take(120).collect::<String>()
    );
}

/// Execute 类任务且**不**命中 [`simple_execute_fast_path`]（含「多个模块」以保留分阶段路径）。
const STAGED_MOCK_EXECUTE_USER: &str =
    "请修复 src/lib.rs 的编译错误，梳理多个模块的依赖关系并运行 cargo test";

fn cfg_staged_execute_turn() -> Arc<AgentConfig> {
    let mut cfg = load_config(None).expect("embedded default config must load");
    cfg.per_plan_policy.planner_executor_mode = PlannerExecutorMode::SingleAgent;
    cfg.intent_routing.intent_at_turn_start_enabled = false;
    cfg.intent_routing.intent_l2_enabled = false;
    cfg.staged_planning.staged_plan_optimizer_round = false;
    cfg.staged_planning.staged_plan_ensemble_count = 1;
    cfg.staged_planning.staged_plan_two_phase_nl_display = false;
    Arc::new(cfg)
}

async fn run_mock_agent_turn(
    cfg: Arc<AgentConfig>,
    messages: &mut Vec<Message>,
    backend: &'static SequencedMockBackend,
) {
    let client = reqwest::Client::new();
    let tools = build_tools();
    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let params = RunAgentTurnParams {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools: tools.as_slice(),
        },
        messages,
        effective_working_dir: work_dir,
        workspace_is_set: true,
        transport: AgentTurnTransport {
            out: None,
            render_to_terminal: false,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            tui_llm_stream_scratch: None,
            tool_running_hook: None,
            clarification_questionnaire_hook: None,
            sse_control_mirror: None,
            llm_backend: Some(backend as &dyn ChatCompletionsBackend),
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
        long_term_memory: None,
        long_term_memory_scope_id: None,
        read_file_turn_cache: None,
        turn_allowed_tool_names: None,
        tracing_chat_turn: None,
        request_audit: None,
        process_handles: ProcessHandles::default_arc_process_handles(),
    };
    run_agent_turn(params)
        .await
        .expect("mock run_agent_turn must succeed");
}

/// 分阶段 `no_task=true`：规划轮后降级 **`run_agent_outer_loop`**（PER 轨），仅再消耗一次 mock LLM。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_staged_no_task_degrades_to_outer_loop() {
    let cfg = cfg_staged_execute_turn();
    let outer_final = "staged no_task mock：外循环终答";
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![
            Message::assistant_only(
                r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#.to_string(),
            ),
            Message::assistant_only(outer_final.to_string()),
        ],
        "stop",
    )));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only(STAGED_MOCK_EXECUTE_USER.to_string()),
    ];
    run_mock_agent_turn(cfg, &mut messages, backend).await;
    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        2,
        "no_task: planner then outer_loop final"
    );
    let last = messages.last().expect("messages");
    let body = message_content_as_str(&last.content).unwrap_or("");
    assert!(
        body.contains(outer_final),
        "expected outer loop final, got {body:?}"
    );
}

/// 分阶段首轮规划解析失败：降级 **`run_agent_outer_loop`**。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_staged_invalid_plan_degrades_to_outer_loop() {
    let cfg = cfg_staged_execute_turn();
    let outer_final = "staged degrade mock：解析失败后外循环终答";
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![
            Message::assistant_only("无结构化 agent_reply_plan 的 planner 正文".to_string()),
            Message::assistant_only(outer_final.to_string()),
        ],
        "stop",
    )));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only(STAGED_MOCK_EXECUTE_USER.to_string()),
    ];
    run_mock_agent_turn(cfg, &mut messages, backend).await;
    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        2,
        "invalid plan: planner degrade then outer_loop final"
    );
    let last = messages.last().expect("messages");
    let body = message_content_as_str(&last.content).unwrap_or("");
    assert!(
        body.contains(outer_final),
        "expected degraded outer final, got {body:?}"
    );
}

/// 分阶段单步：规划 JSON → 步内外循环工具轮 → 步内终答（mock LLM 序列）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_staged_single_step_mock_llm_sequence() {
    let cfg = cfg_staged_execute_turn();
    let client = reqwest::Client::new();
    let tools = build_tools();
    let plan_json = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"调用 get_current_time 查询时间"}]}"#;
    let tool_round = Message {
        role: "assistant".to_string(),
        content: None,
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_staged_1".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "get_current_time".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        name: None,
        tool_call_id: None,
    };
    let step_final = Message::assistant_only("分阶段步骤 mock 终答".to_string());
    let staged_tail_plan = Message::assistant_only(
        r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#.to_string(),
    );
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![
            Message::assistant_only(plan_json.to_string()),
            tool_round,
            step_final.clone(),
            step_final.clone(),
            staged_tail_plan,
        ],
        "stop",
    )));

    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only(STAGED_MOCK_EXECUTE_USER.to_string()),
    ];
    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let params = RunAgentTurnParams {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools: tools.as_slice(),
        },
        messages: &mut messages,
        effective_working_dir: work_dir,
        workspace_is_set: true,
        transport: AgentTurnTransport {
            out: None,
            render_to_terminal: false,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            tui_llm_stream_scratch: None,
            tool_running_hook: None,
            clarification_questionnaire_hook: None,
            sse_control_mirror: None,
            llm_backend: Some(backend as &dyn ChatCompletionsBackend),
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
        long_term_memory: None,
        long_term_memory_scope_id: None,
        read_file_turn_cache: None,
        turn_allowed_tool_names: None,
        tracing_chat_turn: None,
        request_audit: None,
        process_handles: ProcessHandles::default_arc_process_handles(),
    };

    run_agent_turn(params)
        .await
        .expect("staged mock turn must succeed");
    assert!(
        backend.call_seq.load(Ordering::SeqCst) >= 2,
        "staged path should invoke planner at least twice (plan + step outer loop)"
    );
}

/// 分阶段步验收失败 → **`patch_replanner`** → 重试步外循环（mock LLM 序列）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_staged_step_verify_fail_patch_replanner_mock() {
    let mut cfg = (*cfg_staged_execute_turn()).clone();
    cfg.staged_planning.staged_plan_patch_max_attempts = 1;
    let cfg = Arc::new(cfg);
    let plan_json = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"调用 get_current_time 查询时间","executor_kind":"patch_write","acceptance":{"expect_stdout_contains":"PATCH_VERIFY_MARKER_XYZ"}}]}"#;
    let patch_plan_json = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"调用 get_current_time 查询时间（补丁后）"}]}"#;
    let outer_fail_no_tool = Message::assistant_only("本轮未调用任何工具。".to_string());
    let step_final = Message::assistant_only("分阶段补丁后 mock 终答".to_string());
    let staged_tail_plan = Message::assistant_only(
        r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#.to_string(),
    );
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![
            Message::assistant_only(plan_json.to_string()),
            outer_fail_no_tool,
            Message::assistant_only(patch_plan_json.to_string()),
            step_final,
            staged_tail_plan,
        ],
        "stop",
    )));
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only(STAGED_MOCK_EXECUTE_USER.to_string()),
    ];
    run_mock_agent_turn(cfg, &mut messages, backend).await;
    let calls = backend.call_seq.load(Ordering::SeqCst);
    assert!(
        calls >= 4,
        "verify fail patch path: expected >=4 LLM calls (plan+outer+patch+retry), got {calls}"
    );
    let patch_feedback = messages.iter().any(|m| {
        m.role == "user"
            && message_content_as_str(&m.content)
                .unwrap_or("")
                .contains("分阶段规划 · 步级反馈")
    });
    assert!(
        patch_feedback,
        "expected patch planner feedback user after step verify fail"
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
    let params = RunAgentTurnParams {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools: tools.as_slice(),
        },
        messages: &mut messages,
        effective_working_dir: work_dir,
        workspace_is_set: true,
        transport: AgentTurnTransport {
            out: None,
            render_to_terminal: false,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            tui_llm_stream_scratch: None,
            tool_running_hook: None,
            clarification_questionnaire_hook: None,
            sse_control_mirror: None,
            llm_backend: Some(backend as &dyn ChatCompletionsBackend),
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
        long_term_memory: None,
        long_term_memory_scope_id: None,
        read_file_turn_cache: None,
        turn_allowed_tool_names: None,
        tracing_chat_turn: None,
        request_audit: None,
        process_handles: ProcessHandles::default_arc_process_handles(),
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

/// 外循环工具后早停：预置完成证据后单轮终答（`decide_post_tools_exit` → **`StopOuterLoop`**）。
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
            "请分析当前项目的目录结构，并调用 get_current_time 工具查询当前时间，然后用一句话总结。".to_string(),
        ),
        Message {
            role: "tool".to_string(),
            content: Some(
                "当前时间：2026-01-01 00:00:00\n退出码：0\n标准输出：\nok".to_string().into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("get_current_time".to_string()),
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
                name: "get_current_time".to_string(),
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
            "请分析当前项目的目录结构，并调用 get_current_time 工具查询当前时间，然后用一句话总结。".to_string(),
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
