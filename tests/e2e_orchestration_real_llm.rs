//! 编排级真实 LLM e2e 测试（Layer 2）。
//!
//! 覆盖 **SingleAgent** 编排模式，通过 [`crabmate::RunAgentTurnParams`] 注入
//! [`build_e2e_backend`](crabmate::crabmate_llm::build_e2e_backend) 构造的录制/回放后端。
//!
//! 运行模式（环境变量）：
//! - **默认（无 `REAL_LLM_E2E`）**：`#[ignore]` 跳过所有用例
//! - `REAL_LLM_E2E=1`：真实 LLM 后端，不录制
//! - `REAL_LLM_E2E=1 CM_E2E_RECORD=1`：真实 LLM 后端 + 录制

use std::path::Path;
use std::sync::Arc;

use crabmate::crabmate_llm::{build_e2e_backend, detect_mode_from_env};
use crabmate::{
    AgentConfig, AgentTurnLlmOverrides, AgentTurnTransport, ChatCompletionsBackend,
    LlmSeedOverride, Message, PlannerExecutorMode, ProcessHandles, RunAgentTurnParams,
    RunAgentTurnSharedInputs, build_tools, load_config, run_agent_turn,
};

fn cfg_single_agent() -> Arc<AgentConfig> {
    let mut cfg = load_config(None).expect("embedded default config must load");
    cfg.per_plan_policy.planner_executor_mode = PlannerExecutorMode::SingleAgent;
    cfg.intent_routing.intent_at_turn_start_enabled = false;
    cfg.intent_routing.intent_l2_enabled = false;
    Arc::new(cfg)
}

/// 构造 e2e 后端并注入 `RunAgentTurnParams`，执行单轮 agent turn。
///
/// `test_name` 用于录制目录（`tests/fixtures/llm_recordings/<test_name>/`）。
async fn run_single_agent_turn(
    test_name: &str,
    cfg: &Arc<AgentConfig>,
    messages: &mut Vec<Message>,
    workspace_is_set: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mode = detect_mode_from_env();
    let recordings_dir = Path::new("tests/fixtures/llm_recordings");

    let backend_box = build_e2e_backend(
        mode,
        Box::new(crabmate::OpenAiCompatBackend),
        recordings_dir,
        test_name,
    )?;
    let backend_ref: &'static (dyn ChatCompletionsBackend + 'static) = Box::leak(backend_box);

    let client = reqwest::Client::new();
    let tools = build_tools();
    let api_key = std::env::var("API_KEY").unwrap_or_default();
    let work_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let params = RunAgentTurnParams {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: &api_key,
            cfg,
            tools: tools.as_slice(),
        },
        messages,
        effective_working_dir: work_dir,
        workspace_is_set,
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
            llm_backend: Some(backend_ref),
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

    run_agent_turn(params).await?;
    Ok(())
}

/// Smoke 测试：SingleAgent 模式 + 简单问候，验证一轮 LLM 调用后能正常结束。
///
/// 默认 `#[ignore]`（不需要 API_KEY）；设置 `REAL_LLM_E2E=1` 时自动启用。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "设置 REAL_LLM_E2E=1 后执行；需先录制或使用真实 LLM"]
async fn e2e_single_agent_smoke() {
    let cfg = cfg_single_agent();
    let mut messages = vec![
        Message::system_only("你是一个有帮助的助手。用中文回答。".to_string()),
        Message::user_only("你好，用一句话介绍自己。".to_string()),
    ];
    let workspace_is_set = true;

    let result = run_single_agent_turn(
        "orch_single_agent_smoke",
        &cfg,
        &mut messages,
        workspace_is_set,
    )
    .await;

    if let Err(ref e) = result {
        // 失败时简单输出（完整 artifact 落盘待 PR-5）
        eprintln!("e2e 编排测试失败: {e}");
    }

    result.expect("run_agent_turn 应成功结束");

    // 验证末条消息为 assistant
    let last = messages.last().expect("完成后应有至少一条消息");
    assert_eq!(
        last.role, "assistant",
        "末条消息应为 assistant，实际 role={:?}",
        last.role
    );
    // 验证有文本内容
    let body = crabmate::message_content_as_str(&last.content)
        .unwrap_or("")
        .to_string();
    assert!(!body.is_empty(), "assistant 回复不应为空");
}

/// Smoke 测试：SingleAgent + 工具调用（get_current_time），验证工具调用后终答正常。
///
/// 默认 `#[ignore]`；设置 `REAL_LLM_E2E=1` 时自动启用。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "设置 REAL_LLM_E2E=1 后执行；需先录制或使用真实 LLM"]
async fn e2e_single_agent_tool_round() {
    let cfg = cfg_single_agent();
    let mut messages = vec![
        Message::system_only(
            "你是一个有帮助的助手。你可以调用 get_current_time 工具查询当前时间。用中文回答。"
                .to_string(),
        ),
        Message::user_only(
            "请调用 get_current_time 工具查询当前时间，然后用一句话总结。".to_string(),
        ),
    ];
    let workspace_is_set = true;

    let result = run_single_agent_turn(
        "orch_single_agent_tool",
        &cfg,
        &mut messages,
        workspace_is_set,
    )
    .await;

    if let Err(ref e) = result {
        eprintln!("e2e 工具调用测试失败: {e}");
    }

    result.expect("run_agent_turn 应成功结束");

    // 验证末条消息为 assistant
    let last = messages.last().expect("完成后应有至少一条消息");
    assert_eq!(
        last.role, "assistant",
        "末条消息应为 assistant，实际 role={:?}",
        last.role
    );
    // 验证有文本内容（工具调用后的终答）
    let body = crabmate::message_content_as_str(&last.content)
        .unwrap_or("")
        .to_string();
    assert!(!body.is_empty(), "工具调用后的终答不应为空");
}
