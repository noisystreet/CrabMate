//! 编排级集成测：通过 [`crabmate::RunAgentTurnParams`] 注入 [`crabmate::llm::ChatCompletionsBackend`]，
//! 钉住 `run_agent_turn` → `run_agent_outer_loop` 的「Planner → 工具 → Planner → 终答」入口链，**不**访问真实网络。
//!
//! 另含分层：
//! - [`crabmate::run_agent_turn`] + **`PlannerExecutorMode::Hierarchical`**：经 `run_hierarchical_agent` →
//!   `runner::run_hierarchical`，与生产入口一致；
//! - 或直接 [`crabmate::agent::hierarchy::runner::run_hierarchical`]（同上三段 LLM，顺序单子目标）。

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
    FunctionCall, LlmSeedOverride, Message, PlannerExecutorMode, RunAgentTurnParams,
    StreamChatParams, ToolCall, build_tools, load_config, message_content_as_str, run_agent_turn,
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
            .cloned()
            .ok_or_else(|| format!("SequencedMockBackend: unexpected LLM call index {idx}"))?;
        Ok((msg, self.finish_reason.to_string()))
    }
}

fn cfg_single_agent_outer_loop() -> Arc<AgentConfig> {
    let mut cfg = load_config(None).expect("embedded default config must load");
    cfg.planner_executor_mode = PlannerExecutorMode::SingleAgent;
    cfg.staged_plan_execution = false;
    cfg.intent_at_turn_start_enabled = false;
    Arc::new(cfg)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_agent_turn_outer_loop_tool_round_then_final_assistant() {
    let cfg = cfg_single_agent_outer_loop();
    let client = reqwest::Client::new();
    let tools = build_tools();
    let mut messages = vec![
        Message::system_only("test system".to_string()),
        Message::user_only(
            "请调用 get_current_time 工具查询当前时间，然后用一句话总结。".to_string(),
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
    let final_msg = Message::assistant_only("已查询：当前时间可用（mock 编排测）。".to_string());
    let backend: &'static SequencedMockBackend = Box::leak(Box::new(SequencedMockBackend::new(
        vec![tool_round, final_msg.clone()],
        "stop",
    )));

    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let params = RunAgentTurnParams {
        client: &client,
        api_key: "",
        cfg: &cfg,
        tools: tools.as_slice(),
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
    };

    run_agent_turn(params)
        .await
        .expect("mock turn must succeed");
    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        2,
        "expected planner → planner after tool"
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
    cfg.planner_executor_mode = PlannerExecutorMode::Hierarchical;
    cfg.staged_plan_execution = false;
    cfg.intent_at_turn_start_enabled = false;
    cfg.intent_l2_enabled = false;
    cfg.enable_llm_routing = Some(true);
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
        llm_backend: backend as &dyn ChatCompletionsBackend,
        client: client.clone(),
        api_key: String::new(),
        working_dir: work_dir.to_path_buf(),
        sse_out: None,
        tools_defs: tools.as_slice(),
        tool_approval_out: None,
        tool_approval_rx: None,
        primary_intent: Some("execute.read_inspect".to_string()),
        secondary_intents: Vec::new(),
        intent_mode_bias_enabled: false,
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
        client: &client,
        api_key: "",
        cfg: &cfg,
        tools: tools.as_slice(),
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
