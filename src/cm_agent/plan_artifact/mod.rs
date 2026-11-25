//! 最终回答中的结构化「规划」产物：从 assistant content 中解析 JSON，替代 `## 规划` 等子串匹配。

mod display;
mod fence;
mod parse;
mod types;
mod validate;

pub use types::{
    AgentReplyPlanV1, PLAN_V1_EXAMPLE_JSON, PLAN_V1_REWRITE_BRIEF_RULES,
    PLAN_V1_REWRITE_EXAMPLE_JSON, PLAN_V1_SCHEMA_RULES, PlanArtifactError, PlanStepExecutorKind,
    PlanStepV1, agent_reply_plan_v1_to_json_string, plan_artifact_error_log_summary,
};

pub use display::{
    format_agent_reply_plan_for_display, strip_agent_reply_plan_fence_blocks_for_display,
};

pub use parse::parse_agent_reply_plan_v1_with_validate_only_binding_ids;
pub use parse::{parse_agent_reply_plan_v1, parse_agent_reply_plan_v1_from_assistant_message};

pub use validate::{
    validate_plan_binds_workflow_validate_nodes, validate_plan_covers_all_workflow_node_ids,
    validate_plan_workflow_node_ids_subset,
};

#[cfg(test)]
pub use validate::validate_agent_reply_plan_v1_with_validate_only_binding_ids;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> String {
        r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"do a"}]}"#
            .to_string()
    }

    #[test]
    fn plan_artifact_error_log_summary_redacts_long_wrong_type() {
        let long = "x".repeat(80);
        let s = plan_artifact_error_log_summary(&PlanArtifactError::WrongType(long.clone()));
        assert!(!s.contains(&long));
        assert!(s.contains("type_len=80"));
    }

    #[test]
    fn validate_plan_covers_all_workflow_node_ids_gate() {
        let ids = vec!["a".to_string(), "b".to_string()];
        let no_link = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "s1".into(),
                description: "x".into(),
                workflow_node_id: None,
                executor_kind: None,
            }],
            no_task: false,
        };
        assert!(validate_plan_covers_all_workflow_node_ids(&no_link, &ids).is_ok());
        let partial = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "s1".into(),
                description: "x".into(),
                workflow_node_id: Some("a".into()),
                executor_kind: None,
            }],
            no_task: false,
        };
        assert!(validate_plan_covers_all_workflow_node_ids(&partial, &ids).is_err());
        let full = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![
                PlanStepV1 {
                    id: "s1".into(),
                    description: "x".into(),
                    workflow_node_id: Some("a".into()),
                    executor_kind: None,
                },
                PlanStepV1 {
                    id: "s2".into(),
                    description: "y".into(),
                    workflow_node_id: Some("b".into()),
                    executor_kind: None,
                },
            ],
            no_task: false,
        };
        assert!(validate_plan_covers_all_workflow_node_ids(&full, &ids).is_ok());
    }

    #[test]
    fn strip_fence_removes_plan_json_keeps_prose() {
        let bad = r#"{"type":"agent_reply_plan","version":1,"steps":[]}"#;
        let content = format!("说明\n```json\n{bad}\n```\n");
        let s = strip_agent_reply_plan_fence_blocks_for_display(&content);
        assert!(s.contains("说明"));
        assert!(!s.contains("agent_reply_plan"));
    }

    #[test]
    fn strip_fence_keeps_streaming_incomplete_plan_inside_fence() {
        let partial =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x""#;
        let content = format!("先说明一句。\n```json\n{partial}");
        let s = strip_agent_reply_plan_fence_blocks_for_display(&content);
        assert!(s.contains("先说明一句"));
        assert!(
            s.contains("agent_reply_plan"),
            "语法未闭合且 inner 非空时仍保留围栏内流式正文（并带伪造收尾 ```，由上层缓冲抑制刷屏）"
        );
    }

    #[test]
    fn parses_fenced_json() {
        let content = format!("说明\n```json\n{}\n```\n", sample_json());
        let p = parse_agent_reply_plan_v1(&content).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].id, "a");
    }

    #[test]
    fn rejects_step_id_bad_syntax() {
        let bad =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":" bad","description":"x"}]}"#;
        let content = format!("```json\n{bad}\n```");
        assert!(parse_agent_reply_plan_v1(&content).is_err());
    }

    #[test]
    fn validate_workflow_node_id_subset() {
        let plan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "s1".into(),
                description: "do".into(),
                workflow_node_id: Some("fmt".into()),
                executor_kind: None,
            }],
            no_task: false,
        };
        assert!(validate_plan_workflow_node_ids_subset(&plan, &["fmt".into()]).is_ok());
        assert!(validate_plan_workflow_node_ids_subset(&plan, &["other".into()]).is_err());
    }

    #[test]
    fn validate_only_binds_nodes_multiset_ok() {
        let plan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![
                PlanStepV1 {
                    id: "s2".into(),
                    description: "b".into(),
                    workflow_node_id: Some("b".into()),
                    executor_kind: None,
                },
                PlanStepV1 {
                    id: "s1".into(),
                    description: "a".into(),
                    workflow_node_id: Some("a".into()),
                    executor_kind: None,
                },
            ],
            no_task: false,
        };
        assert!(
            validate_plan_binds_workflow_validate_nodes(&plan, &["a".into(), "b".into()]).is_ok()
        );
    }

    #[test]
    fn validate_only_binds_duplicate_nodes() {
        let plan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![
                PlanStepV1 {
                    id: "s1".into(),
                    description: "x".into(),
                    workflow_node_id: Some("dup".into()),
                    executor_kind: None,
                },
                PlanStepV1 {
                    id: "s2".into(),
                    description: "y".into(),
                    workflow_node_id: Some("dup".into()),
                    executor_kind: None,
                },
            ],
            no_task: false,
        };
        assert!(
            validate_plan_binds_workflow_validate_nodes(&plan, &["dup".into(), "dup".into()])
                .is_ok()
        );
        assert!(validate_plan_binds_workflow_validate_nodes(&plan, &["dup".into()]).is_err());
    }

    #[test]
    fn validate_only_requires_workflow_node_id_each_step() {
        let plan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![
                PlanStepV1 {
                    id: "s1".into(),
                    description: "a".into(),
                    workflow_node_id: Some("a".into()),
                    executor_kind: None,
                },
                PlanStepV1 {
                    id: "s2".into(),
                    description: "b".into(),
                    workflow_node_id: None,
                    executor_kind: None,
                },
            ],
            no_task: false,
        };
        assert!(
            validate_plan_binds_workflow_validate_nodes(&plan, &["a".into(), "b".into()]).is_err()
        );
    }

    #[test]
    fn parses_fenced_markdown_wrapped_plan_json() {
        let content = format!("说明\n```markdown\n{}\n```\n", sample_json());
        let p = parse_agent_reply_plan_v1(&content).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].id, "a");
    }

    #[test]
    fn strip_fence_removes_plan_json_markdown_fence() {
        let j = sample_json();
        let content = format!("说明\n```markdown\n{j}\n```\n");
        let s = strip_agent_reply_plan_fence_blocks_for_display(&content);
        assert!(s.contains("说明"));
        assert!(!s.contains("agent_reply_plan"));
        assert!(!s.contains("```"));
    }

    #[test]
    fn strip_fence_unclosed_opening_does_not_emit_six_backticks() {
        let s = strip_agent_reply_plan_fence_blocks_for_display("说明\n```");
        assert_eq!(s, "说明\n");
        assert!(!s.contains("```"));
    }

    #[test]
    fn parses_raw_json_only_message() {
        let p = parse_agent_reply_plan_v1(&sample_json()).unwrap();
        assert_eq!(p.plan_type, "agent_reply_plan");
    }

    #[test]
    fn parses_executor_kind_on_step() {
        let j = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"r","description":"审","executor_kind":"review_readonly"}]}"#;
        let p = parse_agent_reply_plan_v1(j).unwrap();
        assert_eq!(
            p.steps[0].executor_kind,
            Some(PlanStepExecutorKind::ReviewReadonly)
        );
    }

    #[test]
    fn plan_v1_example_json_is_single_test_runner() {
        let p = parse_agent_reply_plan_v1(PLAN_V1_EXAMPLE_JSON).expect("PLAN_V1_EXAMPLE_JSON");
        assert_eq!(p.steps.len(), 1);
        let step = &p.steps[0];
        assert_eq!(step.id, "verify-cargo-check");
        assert_eq!(step.executor_kind, Some(PlanStepExecutorKind::TestRunner));
    }

    #[test]
    fn rejects_legacy_heading() {
        let content = "## 规划\n- step one";
        assert!(parse_agent_reply_plan_v1(content).is_err());
    }

    #[test]
    fn rejects_wrong_type() {
        let s = r#"{"type":"other","version":1,"steps":[{"id":"x","description":"y"}]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn rejects_empty_steps() {
        let s = r#"{"type":"agent_reply_plan","version":1,"steps":[]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn rejects_more_than_one_step() {
        let s = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x"},{"id":"b","description":"y"}]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn allows_multi_steps_when_each_step_has_workflow_node_id() {
        let s = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x","workflow_node_id":"n1"},{"id":"b","description":"y","workflow_node_id":"n2"}]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn allows_multi_steps_when_each_step_has_workflow_node_id_and_binding_context_present() {
        let s = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x","workflow_node_id":"n1"},{"id":"b","description":"y","workflow_node_id":"n2"}]}"#;
        let ids = vec!["n1".to_string(), "n2".to_string()];
        assert!(
            parse_agent_reply_plan_v1_with_validate_only_binding_ids(s, Some(ids.as_slice()))
                .is_ok()
        );
    }

    #[test]
    fn parses_no_task_empty_steps() {
        let s = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let p = parse_agent_reply_plan_v1(s).unwrap();
        assert!(p.no_task);
        assert!(p.steps.is_empty());
    }

    #[test]
    fn parse_agent_reply_plan_v1_from_assistant_message_merges_reasoning_field() {
        let j = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let msg = crate::cm_types::Message {
            role: "assistant".into(),
            content: Some(crate::cm_types::MessageContent::Text(String::new())),
            reasoning_content: Some(j.into()),
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let p = parse_agent_reply_plan_v1_from_assistant_message(&msg).unwrap();
        assert!(p.no_task);
    }

    #[test]
    fn rejects_no_task_with_non_empty_steps() {
        let s = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[{"id":"a","description":"x"}]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn rejects_bad_json_in_fence_then_accepts_second() {
        let content = r#"
```json
not json
```
```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"1","description":"ok"}]}
```
"#;
        assert!(parse_agent_reply_plan_v1(content).is_ok());
    }

    #[test]
    fn format_display_includes_goal_and_steps() {
        let content = "先调研再改代码。\n```json\n{\"type\":\"agent_reply_plan\",\"version\":1,\"steps\":[{\"id\":\"s1\",\"description\":\"读 README\"}]}\n```\n";
        let s = format_agent_reply_plan_for_display(content).expect("formatted");
        assert!(s.contains("调研"));
        assert!(s.contains("1. `s1`: 读 README"));
        assert!(!s.contains("agent_reply_plan"));
    }

    #[test]
    fn format_display_raw_json_only_still_works() {
        let content =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"do"}]}"#;
        let s = format_agent_reply_plan_for_display(content).expect("formatted");
        assert_eq!(s, "1. `a`: do");
    }
}
