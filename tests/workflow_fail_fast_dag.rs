//! `workflow_execute` 在 `fail_fast` 且首节点失败后须能结束调度（回归 P0 空转）。

use crabmate::agent::workflow::{WorkflowApprovalMode, run_workflow_execute_tool};
use crabmate::load_config;

#[tokio::test]
async fn workflow_fail_fast_marks_downstream_skipped_and_returns() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = load_config(None).expect("default config");
    let args = r#"{
        "workflow": {
            "max_parallelism": 1,
            "fail_fast": true,
            "compensate_on_failure": false,
            "nodes": [
                {
                    "id": "fail",
                    "tool_name": "calc",
                    "tool_args": {"expression": "not_a_valid_bc_expr___"},
                    "deps": []
                },
                {
                    "id": "downstream",
                    "tool_name": "get_current_time",
                    "tool_args": {},
                    "deps": ["fail"]
                }
            ]
        }
    }"#;

    let (json, _ws) = run_workflow_execute_tool(
        args,
        &cfg,
        tmp.path(),
        true,
        WorkflowApprovalMode::NoApproval,
        8192,
        None,
    )
    .await;

    let v: serde_json::Value = serde_json::from_str(&json).expect("report json");
    assert_eq!(
        v.get("type").and_then(|x| x.as_str()),
        Some("workflow_execute_result")
    );
    let nodes = v
        .get("nodes")
        .and_then(|x| x.as_array())
        .expect("nodes array");
    let downstream = nodes
        .iter()
        .find(|n| n.get("id").and_then(|x| x.as_str()) == Some("downstream"))
        .expect("downstream node report");
    assert_eq!(
        downstream.get("status").and_then(|x| x.as_str()),
        Some("skipped"),
        "downstream should be skipped under fail_fast: {downstream}"
    );
}
