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

    #[test]
    fn planner_messages_logical_dual_keeps_tools_in_last_step_window() {
        let src = vec![
            Message::user_only("编译"),
            Message::user_staged_step_injection("### 分步 1/1\n- id: s1\n- 描述: build"),
            assistant_body("running"),
            Message {
                role: "tool".to_string(),
                content: Some(MessageContent::Text("make ok".to_string())),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some("run_command".to_string()),
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let out = build_logical_dual_planner_messages(&src, "plan sys".to_string(), false, false);
        assert!(out.iter().any(|m| m.role == "tool"));
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
                turn_budget: crate::agent::turn_budget::TurnBudgetCounter::new_shared(),
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
                    web_tool_ctx: Some(&web),
                    cli_tool_ctx: None,
                    per_flight: None,
                    long_term_memory: None,
                    long_term_memory_scope_id: None,
                    mcp_turn: None,
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
                turn_budget: crate::agent::turn_budget::TurnBudgetCounter::new_shared(),
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
                turn_budget: crate::agent::turn_budget::TurnBudgetCounter::new_shared(),
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
            "请修复 src/lib.rs 的编译错误，梳理多个模块的依赖关系并运行 cargo test",
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
    fn advisory_refactor_consultation_still_allows_staged_when_advisory_bypass_enabled() {
        let mut cfg = test_cfg();
        // 默认 L1 对该句常为 ConfirmThenExecute（不进入分阶段）；下调高阈值以稳定得到 Execute，从而专门验证「咨询启发式」分支。
        cfg.intent_routing.intent_non_hier_execute_high_threshold = 0.35;
        cfg.staged_planning.staged_plan_intent_gate_advisory_bypass = true;
        let messages = vec![Message::user_only(
            "我想对它进行重构，哪些地方隐式状态比较严重，需要重构",
        )];
        let gate = assess_staged_planning_gate(&messages, &cfg);
        assert!(
            gate.allows_staged_planning(),
            "咨询绕过已移除：开启 advisory_bypass 时，Execute 路径下仍应进入滚动分阶段规划"
        );
        match gate {
            StagedPlanningGateOutcome::Allow {
                task_preview: _,
                intent_kind: _,
                primary_intent: _,
                confidence: _,
                decision: _,
            } => {
                // 咨询绕过已从分阶段资格判定中移除，所有 Execute 任务均进入分阶段规划
            }
            other => panic!("unexpected gate outcome: {:?}", other),
        }
    }

    #[test]
    fn advisory_refactor_consultation_allows_staged_planning_by_default() {
        let mut cfg = test_cfg();
        cfg.intent_routing.intent_non_hier_execute_high_threshold = 0.35;
        let messages = vec![Message::user_only(
            "我想对它进行重构，哪些地方隐式状态比较严重，需要重构",
        )];
        let gate = assess_staged_planning_gate(&messages, &cfg);
        assert!(
            gate.allows_staged_planning(),
            "默认关闭咨询绕过时，Execute + 咨询启发式仍应进入分阶段"
        );
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

mod multi_turn_orchestration_fixture_tests {
    use crate::agent::agent_turn::params::{RunLoopTurnState, TurnPlannerHints};
    use crate::agent::agent_turn::task_level_evidence::{
        GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
    };
    use crate::agent::agent_turn::turn_completion::{
        plan_steps_are_redundant_after_completion, tool_calls_are_redundant_after_completion,
    };
    use crate::agent::plan_optimizer::staged_plan_trigger_user_content;
    use crate::types::{FunctionCall, LlmSeedOverride, Message, ToolCall};

    fn msg(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(text.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn tool_env(name: &str, output: &str) -> Message {
        let parsed = crate::tool_result::parse_legacy_output(name, output);
        msg(
            "tool",
            &crate::tool_result::encode_tool_message_envelope_v1(
                name,
                name.to_string(),
                &parsed,
                output,
                None,
            ),
        )
    }

    #[test]
    fn multi_turn_readonly_then_compile_keeps_active_goal_and_suppression_off() {
        let messages = vec![
            msg("user", "分析当前目录"),
            tool_env("list_tree", "list tree: ."),
            msg(
                "assistant",
                "当前目录包含三个压缩包与归档文件，分析结果如下。",
            ),
            msg("user", "编译 hpcg"),
            msg("assistant", "好的，开始编译。"),
        ];
        assert_eq!(
            staged_plan_trigger_user_content(&messages),
            Some("编译 hpcg")
        );
        assert_eq!(
            check_active_user_goal_completion_evidence(&messages),
            GoalCompletionEvidenceCheck::NotApplicable
        );
        assert!(plan_steps_are_redundant_after_completion(&[
            crate::agent::plan_artifact::PlanStepV1 {
                id: "verify".into(),
                description: "检查产物是否存在".into(),
                workflow_node_id: None,
                executor_kind: None,
                step_kind: Some("verify".into()),
                acceptance: None,
                max_step_retries: None,
                transitions: None,
            }
        ]));
    }

    #[test]
    fn orchestration_correction_user_does_not_replace_active_goal() {
        let messages = vec![
            msg("user", "编译 hpcg"),
            msg("user", "【编排纠偏】请实际执行 make"),
        ];
        assert_eq!(
            staged_plan_trigger_user_content(&messages),
            Some("编译 hpcg")
        );
    }

    #[test]
    fn immutable_snapshot_aligns_with_latest_real_user() {
        use crate::agent::agent_turn::errors::AgentTurnSubPhase;

        let mut storage = vec![
            Message::user_only("分析当前目录"),
            Message::assistant_only("完成"),
            Message::user_only("编译 hpcg"),
        ];
        let mut turn = RunLoopTurnState {
            messages_buf: &mut storage,
            messages_revision: 0,
            sub_phase: AgentTurnSubPhase::Planner,
            turn_planner_hints: TurnPlannerHints::default(),
            temperature_override: None,
            model_override: None,
            use_executor_model: false,
            executor_model_override: None,
            executor_api_base: None,
            executor_api_key: None,
            seed_override: LlmSeedOverride::FromConfig,
            turn_budget: crate::agent::turn_budget::TurnBudgetCounter::new_shared(),
        };
        assert_eq!(
            turn.staged_immutable_user_goal_snapshot(),
            Some("编译 hpcg")
        );
    }

    #[test]
    fn redundant_probe_tools_detected_for_completed_outer_loop() {
        let tool_calls = vec![ToolCall {
            id: "tc1".into(),
            typ: "function".into(),
            function: FunctionCall {
                name: "list_tree".into(),
                arguments: "{}".into(),
            },
        }];
        assert!(tool_calls_are_redundant_after_completion(&tool_calls));
    }
}
