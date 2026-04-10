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
        let out = build_single_agent_planner_messages(&src, "plan sys".to_string(), false);
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
        let out = build_logical_dual_planner_messages(&src, "plan sys".to_string(), false);
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

mod per_reflect_tests {
    use std::path::Path;
    use std::sync::Arc;

    use crate::agent::agent_turn::RunLoopParams;
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
            llm_backend: &OPENAI_COMPAT_BACKEND,
            client: &client,
            api_key: "",
            cfg: &cfg,
            tools_defs: &[],
            messages: &mut messages,
            out: None,
            effective_working_dir: Path::new("."),
            workspace_is_set: false,
            no_stream: true,
            cancel: None,
            render_to_terminal: false,
            plain_terminal_stream: false,
            web_tool_ctx: None,
            cli_tool_ctx: None,
            per_flight: None,
            temperature_override: None,
            seed_override: LlmSeedOverride::FromConfig,
            long_term_memory: None,
            long_term_memory_scope_id: None,
            mcp_session: None,
            read_file_turn_cache: None,
            workspace_changelist: None,
            staged_plan_optimizer_round: false,
            staged_plan_optimizer_requires_parallel_tools: true,
            staged_plan_ensemble_count: 1,
            staged_plan_skip_ensemble_on_casual_prompt: true,
            request_chrome_trace: None,
            step_executor_constraint: None,
            turn_allowed_tool_names: None,
        };
        let out = per_reflect_after_assistant(&mut p, &mut c, "stop", &msg).await;
        assert!(matches!(
            out,
            ReflectOnAssistantOutcome::ProceedToExecuteTools
        ));
    }
}
