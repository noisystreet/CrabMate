use super::*;
use crabmate_types::{FunctionCall, MessageContent, ToolCall};

fn test_cfg() -> AgentConfig {
    crabmate_config::load_config(None).expect("embed default config")
}

#[test]
fn per_coordinator_init_from_agent_config_matches_cfg_fields() {
    let cfg = test_cfg();
    let i = PerCoordinatorInit::from_agent_config(&cfg);
    assert_eq!(
        i.reflection_default_max_rounds,
        cfg.per_plan_policy.reflection_default_max_rounds
    );
    assert_eq!(
        i.final_plan_policy,
        cfg.per_plan_policy.final_plan_requirement
    );
    assert_eq!(
        i.plan_rewrite_max_attempts,
        cfg.per_plan_policy.plan_rewrite_max_attempts
    );
    // L1 硬编码：staged_planning 配置字段已删除，默认值为 2
    assert_eq!(i.staged_plan_patch_max_attempts_config, 2);
    assert_eq!(
        i.final_plan_require_strict_workflow_node_coverage,
        cfg.per_plan_policy
            .final_plan_require_strict_workflow_node_coverage
    );
    assert_eq!(
        i.final_plan_semantic_check_enabled,
        cfg.per_plan_policy.final_plan_semantic_check_enabled
    );
    assert_eq!(
        i.final_plan_semantic_check_max_non_readonly_tools,
        cfg.per_plan_policy
            .final_plan_semantic_check_max_non_readonly_tools
    );
}

fn pc(policy: FinalPlanRequirementMode, plan_rewrite_max: usize) -> PerCoordinator {
    PerCoordinator::new(PerCoordinatorInit {
        reflection_default_max_rounds: 5,
        final_plan_policy: policy,
        plan_rewrite_max_attempts: plan_rewrite_max,
        staged_plan_patch_max_attempts_config: 3,
        final_plan_require_strict_workflow_node_coverage: false,
        final_plan_semantic_check_enabled: false,
        final_plan_semantic_check_max_non_readonly_tools: 0,
    })
}

#[test]
fn final_assistant_rewrites_then_stops() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
    let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let _ = c.prepare_workflow_execute(wf_args);
    let empty = Message {
        role: "assistant".to_string(),
        content: Some(MessageContent::Text("no plan here".to_string())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    let hist: Vec<Message> = vec![];
    assert!(matches!(
        c.after_final_assistant(&empty, &hist, &cfg, false),
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
    assert!(matches!(
        c.after_final_assistant(&empty, &hist, &cfg, false),
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
    assert!(matches!(
        c.after_final_assistant(&empty, &hist, &cfg, false),
        AfterFinalAssistant::StopTurnPlanRewriteExhausted {
            reason: PlanRewriteExhaustedReason::PlanMissing
        }
    ));
}

#[test]
fn final_assistant_stops_when_plan_present() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
    let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let _ = c.prepare_workflow_execute(wf_args);
    let hist_one_node = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc0".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":1},"nodes":[{"id":"only"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc0".to_string()),
            },
        ];
    let ok = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"step","workflow_node_id":"only"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
    assert!(matches!(
        c.after_final_assistant(&ok, &hist_one_node, &cfg, false),
        AfterFinalAssistant::StopTurn
    ));
}

#[test]
fn plan_semantics_requires_enough_steps_vs_validate_layers() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 3);
    let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let _ = c.prepare_workflow_execute(wf_args);
    let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3},"nodes":[{"id":"a"},{"id":"b"},{"id":"c"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
    let one_step = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"only","description":"only one step here"}]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&one_step, &hist, &cfg, false),
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
    let three_steps = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[
  {"id":"s0","description":"layer 0","workflow_node_id":"a"},
  {"id":"s1","description":"layer 1","workflow_node_id":"b"},
  {"id":"s2","description":"layer 2","workflow_node_id":"c"}
]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&three_steps, &hist, &cfg, false),
        AfterFinalAssistant::StopTurn
    ));
}

#[test]
fn plan_rewrite_exhausted_reason_layer_mismatch() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 1);
    let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let _ = c.prepare_workflow_execute(wf_args);
    let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3},"nodes":[{"id":"a"},{"id":"b"},{"id":"c"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
    let one_step = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"only","description":"only one step here"}]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&one_step, &hist, &cfg, false),
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
    assert!(matches!(
        c.after_final_assistant(&one_step, &hist, &cfg, false),
        AfterFinalAssistant::StopTurnPlanRewriteExhausted {
            reason: PlanRewriteExhaustedReason::PlanLayerCountMismatch
        }
    ));
}

#[test]
fn plan_workflow_node_id_must_match_last_workflow_nodes() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 3);
    let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let _ = c.prepare_workflow_execute(wf_args);
    let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":1},"nodes":[{"id":"fmt"},{"id":"test"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
    let bad_link = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"step1","description":"run fmt","workflow_node_id":"no-such-node"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
    assert!(matches!(
        c.after_final_assistant(&bad_link, &hist, &cfg, false),
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
    let ok_link = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[
  {"id":"step1","description":"run fmt","workflow_node_id":"fmt"},
  {"id":"step2","description":"run test","workflow_node_id":"test"}
]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&ok_link, &hist, &cfg, false),
        AfterFinalAssistant::StopTurn
    ));
}

#[test]
fn validate_only_duplicate_nodes_plan_must_repeat_workflow_node_id() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 3);
    let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let _ = c.prepare_workflow_execute(wf_args);
    let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":1},"nodes":[{"id":"dup"},{"id":"dup"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
    let one_step_dup = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"both","workflow_node_id":"dup"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
    assert!(matches!(
        c.after_final_assistant(&one_step_dup, &hist, &cfg, false),
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
    let two_steps_dup = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[
  {"id":"s1","description":"first","workflow_node_id":"dup"},
  {"id":"s2","description":"second","workflow_node_id":"dup"}
]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&two_steps_dup, &hist, &cfg, false),
        AfterFinalAssistant::StopTurn
    ));
}

#[test]
fn strict_workflow_coverage_requires_all_nodes_when_any_workflow_node_id() {
    let cfg = test_cfg();
    let mut c = PerCoordinator::new(PerCoordinatorInit {
        reflection_default_max_rounds: 5,
        final_plan_policy: FinalPlanRequirementMode::WorkflowReflection,
        plan_rewrite_max_attempts: 3,
        staged_plan_patch_max_attempts_config: 3,
        final_plan_require_strict_workflow_node_coverage: true,
        final_plan_semantic_check_enabled: false,
        final_plan_semantic_check_max_non_readonly_tools: 0,
    });
    let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let _ = c.prepare_workflow_execute(wf_args);
    let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":1},"nodes":[{"id":"fmt"},{"id":"test"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
    let partial = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"only fmt","workflow_node_id":"fmt"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
    assert!(matches!(
        c.after_final_assistant(&partial, &hist, &cfg, false),
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
    let both = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[
  {"id":"s1","description":"fmt","workflow_node_id":"fmt"},
  {"id":"s2","description":"test","workflow_node_id":"test"}
]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&both, &hist, &cfg, false),
        AfterFinalAssistant::StopTurn
    ));
}

#[test]
fn prepare_workflow_first_round_injects_plan_next() {
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
    let args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let prep = c.prepare_workflow_execute(args);
    assert!(prep.execute);
    assert!(prep.skipped_result.is_empty());
    let ty = prep
        .reflection_inject
        .as_ref()
        .and_then(|v| v.get("instruction_type"))
        .and_then(|x| x.as_str());
    assert_eq!(
        ty,
        Some(workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT)
    );
}

#[test]
fn never_policy_skips_plan_rewrite_even_after_workflow_reflection_inject() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::Never, 2);
    let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let _ = c.prepare_workflow_execute(wf_args);
    let empty = Message {
        role: "assistant".to_string(),
        content: Some(MessageContent::Text("no plan here".to_string())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&empty, &[], &cfg, false),
        AfterFinalAssistant::StopTurn
    ));
}

#[test]
fn always_policy_requests_rewrite_without_workflow() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::Always, 2);
    let empty = Message {
        role: "assistant".to_string(),
        content: Some(MessageContent::Text("no plan".to_string())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&empty, &[], &cfg, false),
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
}

#[test]
fn always_policy_stops_when_plan_present() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::Always, 2);
    let ok = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"Here is my plan:
```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"do the thing"}]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&ok, &[], &cfg, false),
        AfterFinalAssistant::StopTurn
    ));
}

#[test]
fn always_policy_exhausts_rewrites_then_stops() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::Always, 1);
    let empty = Message {
        role: "assistant".to_string(),
        content: Some(MessageContent::Text("no plan at all".to_string())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&empty, &[], &cfg, false),
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
    assert!(matches!(
        c.after_final_assistant(&empty, &[], &cfg, false),
        AfterFinalAssistant::StopTurnPlanRewriteExhausted {
            reason: PlanRewriteExhaustedReason::PlanMissing
        }
    ));
}

#[test]
fn workflow_reflection_no_inject_means_no_requirement() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
    let empty = Message {
        role: "assistant".to_string(),
        content: Some(MessageContent::Text("no plan".to_string())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert!(matches!(
        c.after_final_assistant(&empty, &[], &cfg, false),
        AfterFinalAssistant::StopTurn
    ));
}

#[test]
fn workflow_reflection_done_true_does_not_set_plan_requirement() {
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
    let wf_args_round1 =
        r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
    let _ = c.prepare_workflow_execute(wf_args_round1);
    assert!(c.require_plan_in_final_flag_snapshot());

    let wf_args_done = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":true}}"#;
    let prep = c.prepare_workflow_execute(wf_args_done);
    assert!(prep.execute);
}

#[test]
fn prepare_workflow_reflection_disabled_passes_through() {
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
    let args = r#"{"workflow":{"nodes":[{"id":"a","tool":"ls"}]}}"#;
    let prep = c.prepare_workflow_execute(args);
    assert!(prep.execute);
    assert!(prep.reflection_inject.is_none());
    assert!(!c.require_plan_in_final_flag_snapshot());
}

#[test]
fn staged_patch_planner_counter_independent_of_plan_rewrite() {
    let mut c = pc(FinalPlanRequirementMode::Never, 2);
    assert_eq!(c.staged_plan_patch_planner_rounds_snapshot(), 0);
    assert_eq!(c.plan_rewrite_attempts_snapshot(), 0);
    c.record_staged_plan_patch_planner_round_completed();
    assert_eq!(c.staged_plan_patch_planner_rounds_snapshot(), 1);
    c.increment_plan_rewrite_attempts();
    assert_eq!(c.plan_rewrite_attempts_snapshot(), 1);
    let footer = c.staged_plan_patch_vs_plan_rewrite_counters_footer();
    assert!(footer.contains("分阶段补丁规划已成功合并轮次=1"));
    assert!(footer.contains("plan_rewrite"));
}

#[test]
fn plan_rewrite_attempts_increments_correctly() {
    let cfg = test_cfg();
    let mut c = pc(FinalPlanRequirementMode::Always, 3);
    let empty = Message {
        role: "assistant".to_string(),
        content: Some(MessageContent::Text("no plan".to_string())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    assert_eq!(c.plan_rewrite_attempts_snapshot(), 0);
    let _ = c.after_final_assistant(&empty, &[], &cfg, false);
    assert_eq!(c.plan_rewrite_attempts_snapshot(), 1);
    let _ = c.after_final_assistant(&empty, &[], &cfg, false);
    assert_eq!(c.plan_rewrite_attempts_snapshot(), 2);
    let _ = c.after_final_assistant(&empty, &[], &cfg, false);
    assert_eq!(c.plan_rewrite_attempts_snapshot(), 3);
    assert!(matches!(
        c.after_final_assistant(&empty, &[], &cfg, false),
        AfterFinalAssistant::StopTurnPlanRewriteExhausted {
            reason: PlanRewriteExhaustedReason::PlanMissing
        }
    ));
}

#[test]
fn append_tool_result_and_reflection_with_inject() {
    let mut msgs: Vec<Message> = vec![];
    let inject = serde_json::json!({
        "instruction_type": "test_instruction",
        "body": "do something"
    });
    let mut c = pc(FinalPlanRequirementMode::Never, 2);
    PerCoordinator::append_tool_result_and_reflection(
        &mut c,
        &mut msgs,
        "tc-99".to_string(),
        "tool output".to_string(),
        Some(inject),
    );
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "tool");
    assert_eq!(
        crabmate_types::message_content_as_str(&msgs[0].content),
        Some("tool output")
    );
    assert_eq!(msgs[0].tool_call_id.as_deref(), Some("tc-99"));
    assert_eq!(msgs[1].role, "user");
    assert!(
        crabmate_types::message_content_as_str(&msgs[1].content)
            .unwrap()
            .contains("test_instruction")
    );
}

#[test]
fn workflow_reflection_next_inject_sets_plan_requirement_source() {
    let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
    assert!(!c.require_plan_in_final_flag_snapshot());
    let mut msgs: Vec<Message> = vec![];
    let inject = serde_json::json!({
        "instruction_type": "workflow_reflection_next",
        "round": 2
    });
    PerCoordinator::append_tool_result_and_reflection(
        &mut c,
        &mut msgs,
        "tc-wf".to_string(),
        "ok".to_string(),
        Some(inject),
    );
    assert!(c.require_plan_in_final_flag_snapshot());
}

#[test]
fn append_tool_result_without_reflection() {
    let mut msgs: Vec<Message> = vec![];
    let mut c = pc(FinalPlanRequirementMode::Never, 2);
    PerCoordinator::append_tool_result_and_reflection(
        &mut c,
        &mut msgs,
        "tc-1".to_string(),
        "result".to_string(),
        None,
    );
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, "tool");
}

#[test]
fn layer_count_from_empty_history() {
    assert_eq!(plan_rewrite::last_workflow_validate_layer_count(&[]), None);
}

#[test]
fn layer_count_from_non_workflow_history() {
    let msgs = vec![
        Message {
            role: "user".to_string(),
            content: Some(MessageContent::Text("hello".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("hi".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    ];
    assert_eq!(
        plan_rewrite::last_workflow_validate_layer_count(&msgs),
        None
    );
}

fn hist_with_validate_layer_3() -> Vec<Message> {
    vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3},"nodes":[{"id":"x"},{"id":"y"},{"id":"z"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ]
}

#[test]
fn workflow_validate_layer_cache_reuses_when_len_unchanged() {
    let mut c = pc(FinalPlanRequirementMode::Never, 2);
    let hist = hist_with_validate_layer_3();
    assert_eq!(c.test_workflow_validate_layer_need(&hist), Some(3));
    assert_eq!(c.test_layer_cache_snapshot(), (Some(3), hist.len()));
    assert_eq!(c.test_workflow_validate_layer_need(&hist), Some(3));
}

#[test]
fn workflow_validate_layer_cache_invalidates_on_context_mutation() {
    let mut c = pc(FinalPlanRequirementMode::Never, 2);
    let mut hist = hist_with_validate_layer_3();
    assert_eq!(c.test_workflow_validate_layer_need(&hist), Some(3));
    hist.pop();
    assert_eq!(c.test_workflow_validate_layer_need(&hist), None);
    assert_eq!(c.test_layer_cache_snapshot(), (None, hist.len()));
}

#[test]
fn workflow_validate_layer_from_crabmate_tool_envelope() {
    let inner = r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3},"nodes":[{"id":"x"},{"id":"y"},{"id":"z"}]}"#;
    let parsed = crabmate_tools::tool_result::parse_legacy_output("workflow_execute", inner);
    let wrapped = crabmate_tools::tool_result::encode_tool_message_envelope_v1(
        "workflow_execute",
        "wf".into(),
        &parsed,
        inner,
        None,
    );
    let mut hist = hist_with_validate_layer_3();
    hist[1].content = Some(wrapped.into());
    assert_eq!(
        plan_rewrite::last_workflow_validate_layer_count(&hist),
        Some(3)
    );
}
