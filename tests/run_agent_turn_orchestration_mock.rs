//! 编排级集成测：通过 [`crabmate::RunAgentTurnParams`] 注入 [`crabmate::llm::ChatCompletionsBackend`]，
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
    FunctionCall, LlmSeedOverride, Message, PlannerExecutorMode, ProcessHandles,
    RunAgentTurnParams, RunAgentTurnSharedInputs, StreamChatParams, ToolCall, build_tools,
    load_config, message_content_as_str, run_agent_turn,
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
