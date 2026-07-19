//! `prepare_messages_for_model` 与规划轮请求拼装组合的回归护栏（不经真实 HTTP）。

use std::sync::Arc;

use crate::agent::context_window::{PrepareMessagesForModelHooks, prepare_messages_for_model};
use crate::agent::per_coord::{PerCoordinator, PerCoordinatorInit};
use crate::llm::OPENAI_COMPAT_BACKEND;
use crate::types::{LlmSeedOverride, Message, message_content_as_str};

use super::super::errors::AgentTurnSubPhase;
use super::super::params::{
    RunLoopAttach, RunLoopCore, RunLoopCtx, RunLoopIo, RunLoopObs, RunLoopParams, RunLoopTurnState,
};
use super::prepare_staged_planner_no_tools_request;
use super::rolling_horizon_facade::build_single_agent_planner_messages;
use super::sse::staged_plan_phase_instruction_default;

#[tokio::test]
async fn prepare_then_build_planner_messages_ends_with_plan_system() {
    let cfg = Arc::new(crate::config::load_config(None).expect("embed default"));
    let client = reqwest::Client::new();
    let mut messages = vec![
        Message::user_only("请在本仓库执行一次 cargo check 并汇报结果"),
        Message::assistant_only(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"运行 cargo check"}]}
```"#,
        ),
    ];
    let mut per = PerCoordinator::new(PerCoordinatorInit::from_agent_config(cfg.as_ref()));
    prepare_messages_for_model(
        &OPENAI_COMPAT_BACKEND,
        &client,
        "",
        cfg.as_ref(),
        &mut messages,
        None,
        PrepareMessagesForModelHooks {
            per_coord_layer_cache: Some(&mut per),
            run_loop_messages_revision: None,
            turn_budget: None,
        },
    )
    .await
    .expect("prepare_messages_for_model");

    let plan_sys = staged_plan_phase_instruction_default();
    let cfg_ref: &crabmate_config::AgentConfig = cfg.as_ref();
    let llm_cfg = crabmate_types::llm_config::LlmConfig {
        llm: cfg_ref.llm.clone(),
        sampling: cfg_ref.llm_sampling.clone(),
        vendor_flags: cfg_ref.llm_vendor_flags.clone(),
        http_retry: cfg_ref.llm_http_retry.clone(),
    };
    let preserve_kimi = crate::llm::llm_vendor_adapter(&llm_cfg.llm.model, &llm_cfg.llm.api_base)
        .preserve_assistant_tool_call_reasoning(&llm_cfg);
    let preserve_deepseek =
        crate::llm::vendor::deepseek_json_output_eligible(&llm_cfg.llm.api_base);
    let built = build_single_agent_planner_messages(
        messages.as_slice(),
        plan_sys.clone(),
        preserve_kimi,
        preserve_deepseek,
    );
    let last = built.last().expect("non-empty planner messages");
    assert_eq!(last.role, "system");
    let body = message_content_as_str(&last.content).unwrap_or("");
    assert!(
        body.contains("agent_reply_plan"),
        "规划 system 应包含 schema 约定片段"
    );
    assert!(
        body.len() >= plan_sys.len().saturating_sub(40),
        "system 正文应接近完整规划轮指令"
    );
}

#[tokio::test]
async fn prepare_staged_planner_no_tools_request_fixture_roundtrip() {
    let cfg = Arc::new(crate::config::load_config(None).expect("embed default"));
    let client = reqwest::Client::new();
    let mut messages = vec![Message::user_only("fixture：分阶段规划请求拼装")];
    let mut per = PerCoordinator::new(PerCoordinatorInit::from_agent_config(cfg.as_ref()));

    let mut p = RunLoopParams {
        ctx: RunLoopCtx {
            core: RunLoopCore {
                llm_backend: &OPENAI_COMPAT_BACKEND,
                client: &client,
                api_key: "",
                cfg: &cfg,
                tools_defs: &[],
                effective_working_dir: std::path::Path::new("."),
                workspace_is_set: false,
            },
            io: RunLoopIo {
                out: None,
                no_stream: true,
                cancel: None,
                cancel_arc: None,
                render_to_terminal: false,
                plain_terminal_stream: false,
                tui_llm_stream_scratch: None,
                tool_running_hook: None,
                clarification_questionnaire_hook: None,
                sse_control_mirror: None,
                sse_encoder: crate::sse::default_encoder(),
            },
            attach: RunLoopAttach {
                web_tool_ctx: None,
                cli_tool_ctx: None,
                per_flight: None,
                long_term_memory: None,
                long_term_memory_scope_id: None,
                mcp_turn: None,
                read_file_turn_cache: None,
                workspace_changelist: None,
                staged_plan_optimizer_round: cfg.staged_planning.staged_plan_optimizer_round,
                staged_plan_optimizer_requires_parallel_tools: cfg
                    .staged_planning
                    .staged_plan_optimizer_requires_parallel_tools,
                staged_plan_ensemble_count: cfg.staged_planning.staged_plan_ensemble_count,
                staged_plan_skip_ensemble_on_casual_prompt: cfg
                    .staged_planning
                    .staged_plan_skip_ensemble_on_casual_prompt,
                turn_allowed_tool_names: None,
            },
            obs: RunLoopObs {
                request_chrome_trace: None,
                tracing_chat_turn: None,
                request_audit: None,
                process_handles:
                    crate::process_handles::ProcessHandles::default_arc_process_handles(),
                trace_sink: None,
            },
        },
        turn: RunLoopTurnState {
            messages_buf: &mut messages,
            messages_revision: 0,
            sub_phase: AgentTurnSubPhase::Planner,
            turn_planner_hints: crate::agent::agent_turn::TurnPlannerHints::default(),
            temperature_override: None,
            model_override: None,
            use_executor_model: false,
            executor_model_override: None,
            executor_api_base: None,
            executor_api_key: None,
            seed_override: LlmSeedOverride::FromConfig,
            turn_budget: crate::agent::turn_budget::TurnBudgetCounter::new_shared(),
        },
    };

    let req = prepare_staged_planner_no_tools_request(
        &mut p,
        &mut per,
        build_single_agent_planner_messages,
    )
    .await
    .expect("prepare_staged_planner_no_tools_request");

    assert!(
        req.messages.iter().any(|m| {
            message_content_as_str(&m.content)
                .is_some_and(|c| c.contains("fixture：分阶段规划请求拼装"))
        }),
        "用户正文应在上下文变换后仍出现在 ChatRequest.messages"
    );
    assert!(
        req.messages.iter().any(|m| {
            m.role == "system"
                && message_content_as_str(&m.content).is_some_and(|c| c.contains("分阶段规划"))
        }),
        "末尾规划 system 应进入 ChatRequest"
    );
    assert!(
        req.messages.iter().any(|m| {
            m.role == "system"
                && message_content_as_str(&m.content)
                    .is_some_and(|c| c.contains("不变层（系统持有"))
        }),
        "规划 system 应附带滚动不变层附录"
    );
}
