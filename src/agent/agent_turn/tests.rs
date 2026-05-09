mod push_assistant_merge_tests {
    use crate::types::{Message, MessageContent, message_content_as_str};

    use super::super::messages::push_assistant_merging_trailing_empty_placeholder;
    use super::super::staged::{
        build_logical_dual_planner_messages, build_single_agent_planner_messages,
    };

    fn empty_assistant() -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text(String::new())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn assistant_body(s: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text(s.to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn replaces_trailing_empty_assistant_placeholder() {
        let mut m = vec![Message::user_only("hi"), empty_assistant()];
        push_assistant_merging_trailing_empty_placeholder(&mut m, assistant_body("plan"));
        assert_eq!(m.len(), 2);
        assert_eq!(message_content_as_str(&m[1].content), Some("plan"));
    }

    #[test]
    fn pushes_when_last_assistant_has_content() {
        let mut m = vec![Message::user_only("hi"), assistant_body("first")];
        push_assistant_merging_trailing_empty_placeholder(&mut m, assistant_body("second"));
        assert_eq!(m.len(), 3);
        assert_eq!(message_content_as_str(&m[2].content), Some("second"));
    }

    #[test]
    fn planner_messages_single_agent_keeps_tool_results() {
        let src = vec![
            Message::system_only("sys"),
            Message::user_only("u1"),
            assistant_body("a1"),
            Message {
                role: "tool".to_string(),
                content: Some(MessageContent::Text("tool out".to_string())),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let out = build_single_agent_planner_messages(&src, "plan sys".to_string(), false, false);
        assert_eq!(out.len(), 5);
        assert_eq!(out[3].role, "tool");
        assert_eq!(out[4].role, "system");
        assert_eq!(message_content_as_str(&out[4].content), Some("plan sys"));
    }

    #[test]
    fn planner_messages_logical_dual_drops_tool_and_empty_assistant() {
        let src = vec![
            Message::system_only("sys"),
            Message::user_only("u1"),
            assistant_body("a1"),
            empty_assistant(),
            Message {
                role: "tool".to_string(),
                content: Some(MessageContent::Text("tool out".to_string())),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let out = build_logical_dual_planner_messages(&src, "plan sys".to_string(), false, false);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].role, "system");
        assert_eq!(out[1].role, "user");
        assert_eq!(out[2].role, "assistant");
        assert_eq!(out[3].role, "system");
        assert_eq!(message_content_as_str(&out[3].content), Some("plan sys"));
        assert!(!out.iter().any(|m| m.role == "tool"));
    }
}

mod dedup_tool_calls_tests {
    use crate::types::{FunctionCall, ToolCall};

    use super::super::execute_tools::dedup_readonly_tool_calls_count;

    fn tc(id: &str, name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: args.to_string(),
            },
        }
    }

    #[test]
    fn no_duplicates() {
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.txt"}"#),
            tc("2", "list_dir", r#"{"path":"."}"#),
        ];
        assert_eq!(dedup_readonly_tool_calls_count(&calls), 2);
    }

    #[test]
    fn identical_calls_deduped() {
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.txt"}"#),
            tc("2", "read_file", r#"{"path":"a.txt"}"#),
            tc("3", "read_file", r#"{"path":"a.txt"}"#),
        ];
        assert_eq!(dedup_readonly_tool_calls_count(&calls), 1);
    }

    #[test]
    fn same_name_different_args_not_deduped() {
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.txt"}"#),
            tc("2", "read_file", r#"{"path":"b.txt"}"#),
        ];
        assert_eq!(dedup_readonly_tool_calls_count(&calls), 2);
    }

    #[test]
    fn mixed_duplicates() {
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.txt"}"#),
            tc("2", "list_dir", r#"{"path":"."}"#),
            tc("3", "read_file", r#"{"path":"a.txt"}"#),
            tc("4", "grep", r#"{"pattern":"foo"}"#),
            tc("5", "list_dir", r#"{"path":"."}"#),
        ];
        assert_eq!(dedup_readonly_tool_calls_count(&calls), 3);
    }

    #[test]
    fn empty_batch() {
        assert_eq!(dedup_readonly_tool_calls_count(&[]), 0);
    }
}

mod hierarchy_runner_params_tests {
    use std::path::Path;
    use std::sync::Arc;

    use tokio::sync::{Mutex, mpsc};

    use crate::agent::agent_turn::{
        RunLoopAttach, RunLoopCore, RunLoopCtx, RunLoopIo, RunLoopObs, RunLoopParams,
        RunLoopTurnState,
    };
    use crate::tool_registry::WebToolRuntime;
    use crate::types::{CommandApprovalDecision, LlmSeedOverride, Message};

    #[test]
    fn maps_fields_without_web_tool_ctx() {
        let cfg = Arc::new(crate::config::load_config(None).expect("embed default"));
        let client = reqwest::Client::new();
        let mut messages = vec![Message::user_only("hi")];
        let p = RunLoopParams {
            ctx: RunLoopCtx {
                core: RunLoopCore {
                    llm_backend: &crate::llm::OPENAI_COMPAT_BACKEND,
                    client: &client,
                    api_key: "test-key",
                    cfg: &cfg,
                    tools_defs: &[],
                    effective_working_dir: Path::new("sub/ws"),
                    workspace_is_set: true,
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
                    staged_plan_optimizer_round: false,
                    staged_plan_optimizer_requires_parallel_tools: true,
                    staged_plan_ensemble_count: 1,
                    staged_plan_skip_ensemble_on_casual_prompt: true,
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
                messages_buf: &mut messages,
                messages_revision: 0,
                sub_phase: crate::agent::agent_turn::AgentTurnSubPhase::Planner,
                turn_planner_hints: crate::agent::agent_turn::TurnPlannerHints::default(),
                temperature_override: None,
                model_override: None,
                use_executor_model: false,
                executor_model_override: None,
                executor_api_base: None,
                executor_api_key: None,
                seed_override: LlmSeedOverride::FromConfig,
            },
        };
        let h = p.hierarchy_runner_params(
            "my task",
            Some("execute.run".into()),
            vec!["a".into(), "b".into()],
        );
        assert_eq!(h.task, "my task");
        assert!(std::ptr::eq(h.cfg, cfg.as_ref()));
        assert_eq!(h.api_key, "test-key");
        assert_eq!(h.working_dir, Path::new("sub/ws").to_path_buf());
        assert!(h.sse_out.is_none());
        assert!(h.tool_approval_out.is_none());
        assert!(h.tool_approval_rx.is_none());
        assert_eq!(h.primary_intent.as_deref(), Some("execute.run"));
        assert_eq!(h.secondary_intents, vec!["a", "b"]);
        assert_eq!(
            h.intent_mode_bias_enabled,
            cfg.intent_routing.intent_mode_bias_enabled
        );
        assert!(std::sync::Arc::ptr_eq(
            &h.process_handles,
            &p.ctx.obs.process_handles
        ));
    }

    #[test]
    fn clones_web_approval_channels_when_web_tool_ctx_present() {
        let cfg = Arc::new(crate::config::load_config(None).expect("embed default"));
        let client = reqwest::Client::new();
        let mut messages = vec![Message::user_only("hi")];
        let (out_tx, _out_rx) = mpsc::channel::<String>(4);
        let (_approval_tx, approval_rx) = mpsc::channel::<CommandApprovalDecision>(4);
        let web = WebToolRuntime {
            out_tx: out_tx.clone(),
            approval_rx_shared: Arc::new(Mutex::new(approval_rx)),
            approval_request_guard: Arc::new(Mutex::new(())),
            persistent_allowlist_shared: Arc::new(Mutex::new(Default::default())),
        };
        let p = RunLoopParams {
            ctx: RunLoopCtx {
                core: RunLoopCore {
                    llm_backend: &crate::llm::OPENAI_COMPAT_BACKEND,
                    client: &client,
                    api_key: "",
                    cfg: &cfg,
                    tools_defs: &[],
                    effective_working_dir: Path::new("."),
                    workspace_is_set: false,
                },
                io: RunLoopIo {
                    out: Some(&out_tx),
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
                    web_tool_ctx: Some(&web),
                    cli_tool_ctx: None,
                    per_flight: None,
                    long_term_memory: None,
                    long_term_memory_scope_id: None,
                    mcp_session: None,
                    read_file_turn_cache: None,
                    workspace_changelist: None,
                    staged_plan_optimizer_round: false,
                    staged_plan_optimizer_requires_parallel_tools: true,
                    staged_plan_ensemble_count: 1,
                    staged_plan_skip_ensemble_on_casual_prompt: true,
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
                messages_buf: &mut messages,
                messages_revision: 0,
                sub_phase: crate::agent::agent_turn::AgentTurnSubPhase::Planner,
                turn_planner_hints: crate::agent::agent_turn::TurnPlannerHints::default(),
                temperature_override: None,
                model_override: None,
                use_executor_model: false,
                executor_model_override: None,
                executor_api_base: None,
                executor_api_key: None,
                seed_override: LlmSeedOverride::FromConfig,
            },
        };
        let h = p.hierarchy_runner_params("t", None, vec![]);
        assert!(h.tool_approval_out.is_some());
        assert!(h.tool_approval_rx.is_some());
        assert!(h.sse_out.is_some());
    }
}

mod per_reflect_tests {
    use std::path::Path;
    use std::sync::Arc;

    use crate::agent::agent_turn::{
        RunLoopAttach, RunLoopCore, RunLoopCtx, RunLoopIo, RunLoopObs, RunLoopParams,
        RunLoopTurnState,
    };
    use crate::agent::per_coord::{FinalPlanRequirementMode, PerCoordinator, PerCoordinatorInit};
    use crate::llm::OPENAI_COMPAT_BACKEND;
    use crate::types::{FunctionCall, LlmSeedOverride, Message, MessageContent, ToolCall};

    use super::super::{ReflectOnAssistantOutcome, per_reflect_after_assistant};

    #[tokio::test]
    async fn proceed_to_tools_when_tool_calls_present_but_finish_reason_stop() {
        let cfg = Arc::new(crate::config::load_config(None).expect("embed default"));
        let client = reqwest::Client::new();
        let mut messages = vec![Message::user_only("x")];
        let mut c = PerCoordinator::new(PerCoordinatorInit {
            reflection_default_max_rounds: 5,
            final_plan_policy: FinalPlanRequirementMode::Never,
            plan_rewrite_max_attempts: 3,
            staged_plan_patch_max_attempts_config: 2,
            final_plan_require_strict_workflow_node_coverage: false,
            final_plan_semantic_check_enabled: false,
            final_plan_semantic_check_max_non_readonly_tools: 0,
        });
        let msg = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("ok".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: Some(vec![ToolCall {
                id: "1".into(),
                typ: "function".into(),
                function: FunctionCall {
                    name: "create_file".into(),
                    arguments: "{}".into(),
                },
            }]),
            name: None,
            tool_call_id: None,
        };
        let mut p = RunLoopParams {
            ctx: RunLoopCtx {
                core: RunLoopCore {
                    llm_backend: &OPENAI_COMPAT_BACKEND,
                    client: &client,
                    api_key: "",
                    cfg: &cfg,
                    tools_defs: &[],
                    effective_working_dir: Path::new("."),
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
                    staged_plan_optimizer_round: false,
                    staged_plan_optimizer_requires_parallel_tools: true,
                    staged_plan_ensemble_count: 1,
                    staged_plan_skip_ensemble_on_casual_prompt: true,
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
                messages_buf: &mut messages,
                messages_revision: 0,
                sub_phase: crate::agent::agent_turn::AgentTurnSubPhase::Planner,
                turn_planner_hints: crate::agent::agent_turn::TurnPlannerHints::default(),
                temperature_override: None,
                model_override: None,
                use_executor_model: false,
                executor_model_override: None,
                executor_api_base: None,
                executor_api_key: None,
                seed_override: LlmSeedOverride::FromConfig,
            },
        };
        let out = per_reflect_after_assistant(&mut p, &mut c, "stop", &msg).await;
        assert!(matches!(
            out,
            ReflectOnAssistantOutcome::ProceedToExecuteTools
        ));
    }
}

mod staged_single_step_rolling_tests {
    use super::super::staged::{
        StagedPlanRunOutcome, simulate_single_step_rolling_horizon_for_test,
    };

    #[test]
    fn single_step_then_no_task_finished_reenters_once_then_converges() {
        let rounds = simulate_single_step_rolling_horizon_for_test(
            &[
                StagedPlanRunOutcome::ContinuePlanning,
                StagedPlanRunOutcome::Finished,
            ],
            64,
        )
        .expect("should finish");
        assert_eq!(
            rounds, 2,
            "应先完成一次单步执行并触发重入，再在下一轮 no_task 收敛退出"
        );
    }

    #[test]
    fn rolling_horizon_stops_when_round_limit_exceeded() {
        let err = simulate_single_step_rolling_horizon_for_test(
            &[StagedPlanRunOutcome::ContinuePlanning; 70],
            64,
        )
        .expect_err("should hit round limit");
        assert!(err.contains("超过上限"));
    }
}

mod staged_intent_gate_tests {
    use super::super::intent::staged_planning_gate::{
        StagedPlanningDenyReason, StagedPlanningGateOutcome, assess_staged_planning_gate,
    };
    use crate::types::Message;

    fn test_cfg() -> crate::config::AgentConfig {
        crate::config::load_config(None).expect("embed default")
    }

    #[test]
    fn plain_qa_should_not_enter_staged_planning() {
        let cfg = test_cfg();
        let messages = vec![Message::user_only("你有哪些技能")];
        let gate = assess_staged_planning_gate(&messages, &cfg);
        assert!(!gate.allows_staged_planning(), "普通问答不应进入分阶段规划");
        match gate {
            StagedPlanningGateOutcome::Deny {
                reason: StagedPlanningDenyReason::IntentPipelineNotExecute,
                task_preview: Some(_),
                intent_decision: Some(_),
            } => {}
            other => panic!("unexpected gate outcome: {:?}", other),
        }
    }

    #[test]
    fn execute_task_should_enter_staged_planning() {
        let cfg = test_cfg();
        let messages = vec![Message::user_only(
            "请修复 src/lib.rs 的编译错误并运行 cargo test",
        )];
        let gate = assess_staged_planning_gate(&messages, &cfg);
        assert!(
            gate.allows_staged_planning(),
            "任务执行类请求应进入分阶段规划"
        );
        assert!(
            matches!(gate, StagedPlanningGateOutcome::Allow { .. }),
            "expected Allow outcome"
        );
    }

    #[test]
    fn empty_messages_yield_empty_task_deny() {
        let cfg = test_cfg();
        let messages: Vec<Message> = Vec::new();
        let gate = assess_staged_planning_gate(&messages, &cfg);
        assert!(!gate.allows_staged_planning());
        match gate {
            StagedPlanningGateOutcome::Deny {
                reason: StagedPlanningDenyReason::EmptyEffectiveTask,
                task_preview: None,
                intent_decision: None,
            } => {}
            other => panic!("unexpected gate outcome: {:?}", other),
        }
    }

    #[test]
    fn advisory_refactor_consultation_bypasses_staged_planning() {
        let mut cfg = test_cfg();
        // 默认 L1 对该句常为 ConfirmThenExecute（不进入分阶段）；下调高阈值以稳定得到 Execute，从而专门验证「咨询启发式」分支。
        cfg.intent_routing.intent_non_hier_execute_high_threshold = 0.35;
        let messages = vec![Message::user_only(
            "我想对它进行重构，哪些地方隐式状态比较严重，需要重构",
        )];
        let gate = assess_staged_planning_gate(&messages, &cfg);
        assert!(
            !gate.allows_staged_planning(),
            "架构/重构咨询在 Execute 路径下不应进入滚动分阶段规划"
        );
        match gate {
            StagedPlanningGateOutcome::Deny {
                reason: StagedPlanningDenyReason::AdvisoryExecuteBypassStaged,
                task_preview: Some(_),
                intent_decision: Some(d),
            } => {
                assert!(
                    matches!(
                        d.action,
                        crate::agent::intent_pipeline::IntentAction::Execute
                    ),
                    "门控拒绝分阶段但仍保留 Execute 决策，便于单 Agent 外循环继续"
                );
            }
            other => panic!("unexpected gate outcome: {:?}", other),
        }
    }
}

mod staged_workflow_binding_context_tests {
    use crate::agent::plan_artifact::parse_agent_reply_plan_v1_with_validate_only_binding_ids;

    fn workflow_bound_two_step_plan_json() -> &'static str {
        r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"step1","workflow_node_id":"node.a"},{"id":"s2","description":"step2","workflow_node_id":"node.b"}]}"#
    }

    #[test]
    fn rejects_workflow_bound_multi_step_without_validate_only_binding_ids() {
        assert!(
            parse_agent_reply_plan_v1_with_validate_only_binding_ids(
                workflow_bound_two_step_plan_json(),
                None
            )
            .is_err()
        );
    }

    #[test]
    fn allows_workflow_bound_multi_step_with_validate_only_binding_ids() {
        let ids = vec!["node.a".to_string(), "node.b".to_string()];
        assert!(
            parse_agent_reply_plan_v1_with_validate_only_binding_ids(
                workflow_bound_two_step_plan_json(),
                Some(ids.as_slice())
            )
            .is_ok()
        );
    }
}
