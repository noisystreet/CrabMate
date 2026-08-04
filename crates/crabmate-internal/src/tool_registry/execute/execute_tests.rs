//! `execute` 模块单元测试（拆出以降低 `execute.rs` 物理行数棘轮）。

use super::super::meta::HandlerLookupTable;
use super::*;
use crabmate_types::{FunctionCall, ToolCall};

fn tool_call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: "tc_1".to_string(),
        typ: "function".to_string(),
        function: FunctionCall {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
    }
}

#[test]
fn read_dir_path_is_external_detects_absolute_and_parent_ref() {
    assert_eq!(
        read_dir_path_is_external(r#"{"path":"/tmp"}"#),
        Some("/tmp".to_string())
    );
    assert_eq!(
        read_dir_path_is_external(r#"{"path":"../secrets"}"#),
        Some("../secrets".to_string())
    );
    assert_eq!(read_dir_path_is_external(r#"{"path":"src"}"#), None);
}

#[tokio::test]
async fn prefetch_parallel_syncdefault_approvals_blocks_external_read_dir_without_channel() {
    let calls = vec![tool_call("read_dir", r#"{"path":"/tmp"}"#)];
    let failures = prefetch_parallel_syncdefault_approvals(
        &calls,
        None,
        None,
        &HandlerLookupTable::default_dispatch(),
    )
    .await;
    assert_eq!(failures.len(), 1);
    let msg = failures
        .get(&("read_dir".to_string(), r#"{"path":"/tmp"}"#.to_string()))
        .expect("missing failure for external read_dir");
    assert!(msg.contains("需要审批通道"));
}

#[tokio::test]
async fn external_run_command_gate_not_needed_when_disabled_or_safe_args() {
    let mut cfg = crabmate_config::load_config(None).expect("embed default");
    let allowed = cfg.command_exec.allowed_commands.to_vec();
    let wd = std::path::Path::new(".");

    cfg.command_exec.allow_external_path_with_approval = false;
    let g = approve_external_run_command_paths_if_needed(
        &cfg,
        r#"{"command":"cat","args":["/etc/passwd"]}"#,
        wd,
        &allowed,
        None,
        None,
        "run_command",
    )
    .await
    .expect("disabled → NotNeeded");
    assert_eq!(g, ExternalPathGate::NotNeeded);

    cfg.command_exec.allow_external_path_with_approval = true;
    let g = approve_external_run_command_paths_if_needed(
        &cfg,
        r#"{"command":"git","args":["log","main..HEAD"]}"#,
        wd,
        &allowed,
        None,
        None,
        "run_command",
    )
    .await
    .expect("git range → NotNeeded");
    assert_eq!(g, ExternalPathGate::NotNeeded);
}

#[tokio::test]
async fn external_run_command_gate_errs_without_channel_or_in_docker() {
    let mut cfg = crabmate_config::load_config(None).expect("embed default");
    cfg.command_exec.allow_external_path_with_approval = true;
    let allowed = cfg.command_exec.allowed_commands.to_vec();
    let wd = std::path::Path::new(".");
    let abs = r#"{"command":"cat","args":["/etc/passwd"]}"#;

    let err = approve_external_run_command_paths_if_needed(
        &cfg,
        abs,
        wd,
        &allowed,
        None,
        None,
        "run_command",
    )
    .await
    .expect_err("no channel");
    assert!(err.contains("需要审批通道"), "{err}");

    cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode =
        crabmate_config::SyncDefaultToolSandboxMode::Docker;
    let err = approve_external_run_command_paths_if_needed(
        &cfg,
        abs,
        wd,
        &allowed,
        None,
        None,
        "run_command",
    )
    .await
    .expect_err("docker");
    assert!(err.contains("Docker"), "{err}");
}
