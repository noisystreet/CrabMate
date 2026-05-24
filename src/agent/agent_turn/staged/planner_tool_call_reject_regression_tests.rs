//! `PLANNER_TOOL_CALL_REJECTED`：首轮规划轮误出 `tool_calls` 时，重写约束 user 仅用于一次重试，不得落盘或进入 Web 快照。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use crate::agent::context_window::{PrepareMessagesForModelHooks, prepare_messages_for_model};
use crate::agent::per_coord::{PerCoordinator, PerCoordinatorInit};
use crate::llm::{ChatCompletionsBackend, StreamChatParams};
use crate::types::{
    FunctionCall, LlmSeedOverride, Message, STAGED_PLANNER_TOOL_CALL_REJECT_CONTENT_PREFIX,
    ToolCall, filter_messages_for_web_client_snapshot, is_planner_tool_call_reject_injection,
    message_content_as_str,
};

use super::super::errors::AgentTurnSubPhase;
use super::super::params::{
    RunLoopAttach, RunLoopCore, RunLoopCtx, RunLoopIo, RunLoopObs, RunLoopParams, RunLoopTurnState,
};
use super::planner_round_driver::complete_first_planner_round_maybe_retry_tool_reject;
use super::{
    StagedPlanRunLabels, build_single_agent_planner_messages,
    prepare_staged_planner_no_tools_request,
};

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
        req: &mut crate::types::ChatRequest,
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

fn tc(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        typ: "function".to_string(),
        function: FunctionCall {
            name: name.to_string(),
            arguments: r#"{"path":"README.md"}"#.to_string(),
        },
    }
}

fn make_run_loop_params<'a>(
    cfg: &'a Arc<crate::config::AgentConfig>,
    client: &'a reqwest::Client,
    backend: &'a SequencedMockBackend,
    messages: &'a mut Vec<Message>,
) -> RunLoopParams<'a> {
    RunLoopParams {
        ctx: RunLoopCtx {
            core: RunLoopCore {
                llm_backend: backend,
                client,
                api_key: "",
                cfg,
                tools_defs: &[],
                effective_working_dir: std::path::Path::new("."),
                workspace_is_set: false,
            },
            io: RunLoopIo {
                out: None,
                no_stream: true,
                cancel: None,
                render_to_terminal: false,
                plain_terminal_stream: false,
                tui_llm_stream_scratch: None,
                tool_running_hook: None,
                clarification_questionnaire_hook: None,
                sse_control_mirror: None,
            },
            attach: RunLoopAttach {
                web_tool_ctx: None,
                cli_tool_ctx: None,
                per_flight: None,
                long_term_memory: None,
                long_term_memory_scope_id: None,
                mcp_session: None,
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
            },
        },
        turn: RunLoopTurnState {
            messages_buf: messages,
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
        },
    }
}

#[tokio::test]
async fn reject_user_ephemeral_after_first_planner_tool_calls_retry() {
    const USER_GOAL: &str = "用户原始诉求：分析当前项目结构";
    let cfg = Arc::new(crate::config::load_config(None).expect("embed default"));
    let client = reqwest::Client::new();
    let planner_with_tools = Message {
        role: "assistant".to_string(),
        content: None,
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![
            tc("c1", "read_file"),
            tc("c2", "list_dir"),
            tc("c3", "list_dir"),
        ]),
        name: None,
        tool_call_id: None,
    };
    let planner_retry_ok = Message::assistant_only(
        r#"```json
{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}
```"#,
    );
    let backend = SequencedMockBackend::new(vec![planner_with_tools, planner_retry_ok], "stop");

    let mut messages = vec![Message::user_only(USER_GOAL)];
    let mut per = PerCoordinator::new(PerCoordinatorInit::from_agent_config(cfg.as_ref()));
    prepare_messages_for_model(
        &backend,
        &client,
        "",
        cfg.as_ref(),
        &mut messages,
        None,
        PrepareMessagesForModelHooks {
            per_coord_layer_cache: Some(&mut per),
            run_loop_messages_revision: None,
        },
    )
    .await
    .expect("prepare_messages_for_model");
    let mut p = make_run_loop_params(&cfg, &client, &backend, &mut messages);

    let labels = StagedPlanRunLabels {
        planning_log_label: "regression::planner_tool_call_reject",
        step_injection_log_label: "regression::step",
        build_planner_messages: build_single_agent_planner_messages,
    };
    let req =
        prepare_staged_planner_no_tools_request(&mut p, &mut per, labels.build_planner_messages)
            .await
            .expect("prepare_staged_planner_no_tools_request");

    let (_msg, _) = complete_first_planner_round_maybe_retry_tool_reject(
        &mut p,
        &mut per,
        &req,
        false,
        labels,
        &Message::user_only,
    )
    .await
    .expect("complete_first_planner_round_maybe_retry_tool_reject");

    assert_eq!(
        backend.call_seq.load(Ordering::SeqCst),
        2,
        "首轮含 tool_calls 应触发一次无工具重写"
    );
    assert!(
        messages.iter().any(|m| {
            m.role == "user"
                && message_content_as_str(&m.content).is_some_and(|c| c.contains(USER_GOAL))
        }),
        "真实用户诉求应仍在缓冲中"
    );
    assert!(
        !messages.iter().any(is_planner_tool_call_reject_injection),
        "重写约束 user 应在 LLM 重试完成后弹出，不得留在 messages 缓冲"
    );
    assert!(
        !messages.iter().any(|m| {
            message_content_as_str(&m.content)
                .is_some_and(|c| c.contains(STAGED_PLANNER_TOOL_CALL_REJECT_CONTENT_PREFIX))
        }),
        "缓冲中不应残留 PLANNER_TOOL_CALL_REJECTED 正文"
    );
    let web_snapshot = filter_messages_for_web_client_snapshot(messages.as_slice());
    assert_eq!(web_snapshot.len(), 1, "Web 快照应仅含真实 user，不含注入条");
    assert!(
        message_content_as_str(&web_snapshot[0].content).is_some_and(|c| c.contains(USER_GOAL)),
        "Web 快照 user 应为用户原文"
    );
}
