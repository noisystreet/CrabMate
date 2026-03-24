use super::*;

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
        },
    ];
    assert!(validate_dag(&nodes).is_err());
}

#[test]
fn test_node_ready() {
    let completed: HashMap<String, NodeRunResult> = HashMap::new();
    let ready = node_ready(&[] as &[String], &completed);
    assert!(ready);
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
            {"id":"a","tool_name":"calc","tool_args":{}}
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
            {"id":"a","tool_name":"calc","tool_args":{},"max_retries":99}
          ]
        }
    }"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes[0].max_retries, 5);
}

#[test]
fn test_parse_max_retries_explicit_value() {
    let json = r#"{
        "workflow":{
          "nodes":[
            {"id":"a","tool_name":"calc","tool_args":{},"max_retries":3}
          ]
        }
    }"#;
    let spec = parse_workflow_spec(json).unwrap();
    assert_eq!(spec.nodes[0].max_retries, 3);
}
