mod push_assistant_merge_tests {
    use crate::types::{Message, MessageContent, message_content_as_str};

    use super::super::messages::push_assistant_merging_trailing_empty_placeholder;

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

mod workflow_binding_context_tests {
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
    use crate::agent::agent_turn::turn_completion::tool_calls_are_redundant_after_completion;
    use crate::types::{FunctionCall, ToolCall};

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
