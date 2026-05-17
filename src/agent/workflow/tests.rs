use std::collections::HashMap;

use super::dag::validate_dag;
use super::model::WorkflowNodeSpec;
use super::parse::parse_workflow_spec;
use super::placeholders::inject_placeholders;
use super::run_if::node_deps_resolved;
use super::types::{NodeRunResult, NodeRunStatus};

#[test]
fn test_parse_workflow_template_rust_ci_light() {
    let json = r#"{"workflow":{"workflow_template":"rust_ci_light"}}"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes.len(), 4);
    assert_eq!(spec.nodes[0].tool_name, "cargo_fmt_check");
    assert_eq!(spec.nodes[1].tool_name, "cargo_check");
    assert_eq!(spec.nodes[2].tool_name, "cargo_clippy");
    assert_eq!(spec.nodes[3].tool_name, "cargo_test");
    assert_eq!(spec.cached_layer_count, 4);
}

#[test]
fn test_parse_workflow_template_overlay_fail_fast() {
    let json = r#"{"workflow":{"workflow_template":"rust_ci_light","fail_fast":false}}"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert!(!spec.fail_fast);
    assert_eq!(spec.nodes.len(), 4);
}

#[test]
fn test_parse_unknown_workflow_template_errors() {
    let json = r#"{"workflow":{"workflow_template":"unknown_xyz"}}"#;
    let err = parse_workflow_spec(json).unwrap_err();
    assert!(err.contains("未知 workflow_template"), "got {err}");
    assert!(err.contains("code_review"), "got {err}");
}

#[test]
fn test_parse_workflow_template_code_review() {
    let json = r#"{"workflow":{"workflow_template":"code_review"}}"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes.len(), 4);
    assert!(!spec.fail_fast);
    assert_eq!(spec.nodes[0].tool_name, "git_diff_names");
    assert_eq!(spec.nodes[1].tool_name, "git_diff");
    assert_eq!(spec.nodes[2].tool_name, "search_in_files");
    assert_eq!(spec.nodes[3].tool_name, "cargo_clippy");
}

#[test]
fn test_parse_workflow_template_refactor_precheck_requires_symbol() {
    let json = r#"{"workflow":{"workflow_template":"refactor_precheck"}}"#;
    let err = parse_workflow_spec(json).unwrap_err();
    assert!(err.contains("refactor_symbol"), "got {err}");
}

#[test]
fn test_parse_workflow_template_refactor_precheck_with_symbol() {
    let json =
        r#"{"workflow":{"workflow_template":"refactor_precheck","refactor_symbol":"MyService"}}"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes.len(), 2);
    assert_eq!(spec.nodes[0].tool_name, "call_graph_sketch");
    assert_eq!(spec.nodes[1].tool_name, "find_references");
    assert_eq!(
        spec.nodes[0]
            .tool_args
            .get("symbol")
            .and_then(|x| x.as_str()),
        Some("MyService")
    );
    assert_eq!(
        spec.nodes[1]
            .tool_args
            .get("symbol")
            .and_then(|x| x.as_str()),
        Some("MyService")
    );
}

#[test]
fn test_parse_workflow_spec_array_nodes() {
    let json = r#"{
        "workflow":{
          "max_parallelism":2,
          "fail_fast":true,
          "compensate_on_failure":true,
          "nodes":[
            {"id":"a","tool_name":"get_current_time","tool_args":{},"deps":[]},
            {"id":"b","tool_name":"calc","tool_args":{"expression":"1+1"},"deps":["a"]}
          ]
        }
    }"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes.len(), 2);
    assert_eq!(spec.nodes[0].id, "a");
    assert_eq!(spec.nodes[1].deps, vec!["a".to_string()]);
}

#[test]
fn test_parse_rejects_calc_without_expression() {
    let json = r#"{
        "workflow":{
          "nodes":[
            {"id":"a","tool_name":"calc","tool_args":{}}
          ]
        }
    }"#;
    let err = parse_workflow_spec(json).unwrap_err();
    assert!(
        err.contains("expression"),
        "expected missing required key in error: {err}"
    );
}

#[test]
fn test_validate_dag_cycle_detection() {
    let nodes = vec![
        WorkflowNodeSpec {
            id: "a".to_string(),
            tool_name: "calc".to_string(),
            tool_args: serde_json::json!({}),
            deps: vec!["b".to_string()],
            requires_approval: false,
            timeout_secs: None,
            compensate_with: vec![],
            max_retries: 0,
            node_tool_role: None,
            run_if: None,
        },
        WorkflowNodeSpec {
            id: "b".to_string(),
            tool_name: "calc".to_string(),
            tool_args: serde_json::json!({}),
            deps: vec!["a".to_string()],
            requires_approval: false,
            timeout_secs: None,
            compensate_with: vec![],
            max_retries: 0,
            node_tool_role: None,
            run_if: None,
        },
    ];
    assert!(validate_dag(&nodes).is_err());
}

#[test]
fn test_node_deps_resolved_empty_deps() {
    let completed: HashMap<String, NodeRunResult> = HashMap::new();
    assert!(node_deps_resolved(&[] as &[String], &completed));
}

#[test]
fn test_inject_placeholders_output_truncation() {
    let mut completed: HashMap<String, NodeRunResult> = HashMap::new();
    completed.insert(
        "a".to_string(),
        NodeRunResult {
            id: "a".to_string(),
            status: NodeRunStatus::Passed,
            output: "hello world".repeat(200).into(),
            workspace_changed: false,
            exit_code: Some(0),
            error_code: None,
            attempt: 1,
        },
    );
    let v = serde_json::json!({"x":"prefix {{a.output}} suffix"});
    let injected = inject_placeholders(&v, &completed, 20);
    let x = injected.get("x").and_then(|y| y.as_str()).unwrap_or("");
    assert!(x.contains("prefix "));
    assert!(x.contains("suffix"));
    assert!(x.len() <= "prefix ".len() + 20 + " suffix".len() + 32); // 允许截断标记冗余
}

#[test]
fn test_placeholder_stdout_first_token() {
    let mut completed: HashMap<String, NodeRunResult> = HashMap::new();
    completed.insert(
        "a".to_string(),
        NodeRunResult {
            id: "a".to_string(),
            status: NodeRunStatus::Passed,
            output: "deadbeef123 some message\nsecond line".into(),
            workspace_changed: false,
            exit_code: Some(0),
            error_code: None,
            attempt: 1,
        },
    );
    let v = serde_json::json!({"rev":"{{a.stdout_first_token}}"});
    let injected = inject_placeholders(&v, &completed, 64);
    let rev = injected.get("rev").and_then(|x| x.as_str()).unwrap_or("");
    assert_eq!(rev, "deadbeef123");
}

#[test]
fn test_parse_max_retries_defaults_to_zero() {
    let json = r#"{
        "workflow":{
          "nodes":[
            {"id":"a","tool_name":"calc","tool_args":{"expression":"1"}}
          ]
        }
    }"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes[0].max_retries, 0);
}

#[test]
fn test_parse_max_retries_capped_at_five() {
    let json = r#"{
        "workflow":{
          "nodes":[
            {"id":"a","tool_name":"calc","tool_args":{"expression":"1"},"max_retries":99}
          ]
        }
    }"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes[0].max_retries, 5);
}

#[test]
fn workflow_retryable_error_codes() {
    use crate::agent::workflow::execute::workflow_node_failure_retryable;
    assert!(workflow_node_failure_retryable(Some("timeout")));
    assert!(workflow_node_failure_retryable(Some(
        "workflow_tool_join_error"
    )));
    assert!(!workflow_node_failure_retryable(Some("cargo_test_failed")));
    assert!(!workflow_node_failure_retryable(None));
}

#[test]
fn test_parse_max_retries_explicit_value() {
    let json = r#"{
        "workflow":{
          "nodes":[
            {"id":"a","tool_name":"calc","tool_args":{"expression":"1"},"max_retries":3}
          ]
        }
    }"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes[0].max_retries, 3);
}

#[test]
fn test_parse_node_tool_role_executor_kind_alias() {
    let json = r#"{
        "workflow":{
          "nodes":[
            {"id":"r","tool_name":"read_file","tool_args":{"path":"README.md"},"executor_kind":"review_readonly"}
          ]
        }
    }"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(
        spec.nodes[0].node_tool_role,
        Some(super::node_tool_role::WorkflowNodeToolRole::ReviewReadonly)
    );
}

#[test]
fn test_compile_workflow_author_yaml_serial_after() {
    let yaml = include_str!("../../../fixtures/workflows/01_serial_after.yaml");
    let compiled = super::compile_spec::compile_workflow_author_yaml(yaml).unwrap();
    let args_json = serde_json::to_string(&compiled).unwrap();
    let spec = parse_workflow_spec(&args_json).unwrap();
    assert_eq!(spec.nodes.len(), 3);
    assert!(spec.fail_fast);
    assert_eq!(spec.nodes[0].id, "diff");
    assert_eq!(spec.nodes[0].tool_name, "git_diff");
    assert!(spec.nodes[0].deps.is_empty());
    assert_eq!(spec.nodes[1].id, "clippy");
    assert_eq!(spec.nodes[1].deps, vec!["diff".to_string()]);
    assert_eq!(spec.nodes[2].deps, vec!["clippy".to_string()]);
    assert_eq!(spec.cached_layer_count, 3);
}

#[test]
fn test_compile_workflow_author_branch_when() {
    let yaml = include_str!("../../../fixtures/workflows/02_branch_when_success_failure.yaml");
    let compiled = super::compile_spec::compile_workflow_author_yaml(yaml).unwrap();
    let nodes = compiled
        .get("workflow")
        .and_then(|w| w.get("nodes"))
        .and_then(|n| n.as_array())
        .unwrap();
    assert_eq!(nodes.len(), 3);
    assert!(nodes[1].get("run_if").is_some());
    assert!(nodes[2].get("run_if").is_some());
}

#[test]
fn test_run_if_branch_success_failure() {
    use super::model::{WorkflowBranch, WorkflowRunIf};
    use super::run_if::node_run_if_satisfied;
    let mut completed: HashMap<String, NodeRunResult> = HashMap::new();
    completed.insert(
        "lint".into(),
        NodeRunResult {
            id: "lint".into(),
            status: NodeRunStatus::Failed,
            output: "err".into(),
            workspace_changed: false,
            exit_code: Some(1),
            error_code: None,
            attempt: 1,
        },
    );
    let on_fail = WorkflowRunIf::Branch {
        from: "lint".into(),
        branch: WorkflowBranch::Failure,
    };
    let on_ok = WorkflowRunIf::Branch {
        from: "lint".into(),
        branch: WorkflowBranch::Success,
    };
    assert!(node_run_if_satisfied(Some(&on_fail), &completed));
    assert!(!node_run_if_satisfied(Some(&on_ok), &completed));
}

#[test]
fn test_compile_static_for_each() {
    let yaml = r#"
version: 2
steps:
  - id: prep
    tool: diagnostic_summary
  - id: each
    tool: read_file
    after: [prep]
    for_each:
      from: prep
      static_items: ["a.rs", "b.rs"]
      max_items: 10
    args:
      path: "{{item}}"
"#;
    let compiled = super::compile_spec::compile_workflow_author_yaml(yaml).unwrap();
    let nodes = compiled["workflow"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[1]["id"], "each_0");
    assert_eq!(nodes[2]["id"], "each_1");
}

#[test]
fn test_parse_workflow_spec_accepts_inline_steps_json() {
    let json = r#"{
        "workflow": { "fail_fast": true },
        "steps": [
            {"id":"a","tool":"git_diff","args":{"mode":"all"}},
            {"id":"b","tool":"cargo_clippy","after":["a"],"args":{}}
        ]
    }"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes.len(), 2);
    assert_eq!(spec.nodes[1].deps, vec!["a".to_string()]);
}

#[test]
fn test_resolve_workflow_execute_args_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let wf_dir = dir.path().join(".crabmate/workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    let wf_path = wf_dir.join("ci.yaml");
    std::fs::write(
        &wf_path,
        r#"version: 2
workflow:
  fail_fast: true
steps:
  - id: a
    tool: diagnostic_summary
"#,
    )
    .unwrap();
    let args = r#"{"workflow_file":".crabmate/workflows/ci.yaml"}"#;
    let resolved =
        super::author_load::resolve_workflow_execute_args(args, dir.path(), true).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resolved).unwrap();
    assert!(v.get("workflow_file").is_none());
    assert!(v.get("workflow").and_then(|w| w.get("nodes")).is_some());
}

#[test]
fn test_md_extract_crabmate_workflow_block() {
    let md = include_str!("../../../fixtures/workflows/09_fenced_in_markdown.md");
    let blocks = super::md_extract::extract_crabmate_workflow_blocks(md);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].contains("rust_ci_light"));
}
