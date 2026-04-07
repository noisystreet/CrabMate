use crate::types::{FunctionCall, ToolCall};

use super::meta::{
    HandlerId, ToolExecutionClass, execution_class_for_tool, handler_id_for, try_dispatch_meta,
};
use super::policy::{
    parallel_tool_wall_timeout_secs, sync_default_runs_inline, tool_calls_allow_parallel_sync_batch,
};

fn tc(name: &str) -> ToolCall {
    ToolCall {
        id: "x".to_string(),
        typ: "function".to_string(),
        function: FunctionCall {
            name: name.to_string(),
            arguments: "{}".to_string(),
        },
    }
}

fn test_cfg() -> crate::config::AgentConfig {
    crate::config::load_config(None).expect("embed default")
}

#[test]
fn parallel_sync_batch_two_readonly_sync_tools() {
    let cfg = test_cfg();
    let batch = vec![tc("read_file"), tc("list_dir")];
    assert!(tool_calls_allow_parallel_sync_batch(&cfg, &batch));
}

#[test]
fn parallel_sync_batch_mixed_readonly_http_and_search() {
    let cfg = test_cfg();
    assert!(tool_calls_allow_parallel_sync_batch(
        &cfg,
        &[tc("read_file"), tc("http_fetch")]
    ));
    assert!(tool_calls_allow_parallel_sync_batch(
        &cfg,
        &[tc("get_weather"), tc("web_search")]
    ));
}

#[test]
fn parallel_sync_batch_denied_for_cargo_or_workflow() {
    let cfg = test_cfg();
    assert!(!tool_calls_allow_parallel_sync_batch(
        &cfg,
        &[tc("read_file"), tc("cargo_check")]
    ));
    assert!(!tool_calls_allow_parallel_sync_batch(
        &cfg,
        &[tc("workflow_execute"), tc("read_file")]
    ));
}

#[test]
fn parallel_sync_batch_denied_for_http_request() {
    let cfg = test_cfg();
    assert!(!tool_calls_allow_parallel_sync_batch(
        &cfg,
        &[tc("read_file"), tc("http_request")]
    ));
}

#[test]
fn parallel_sync_batch_single_tool_false() {
    let cfg = test_cfg();
    assert!(!tool_calls_allow_parallel_sync_batch(
        &cfg,
        &[tc("read_file")]
    ));
}

#[test]
fn handler_map_resolves_known_tools() {
    assert_eq!(handler_id_for("workflow_execute"), HandlerId::Workflow);
    assert_eq!(handler_id_for("run_command"), HandlerId::RunCommand);
    assert_eq!(handler_id_for("web_search"), HandlerId::WebSearch);
    assert_eq!(handler_id_for("http_request"), HandlerId::HttpRequest);
    assert_eq!(handler_id_for("unknown_xyz"), HandlerId::SyncDefault);
}

#[test]
fn try_dispatch_meta_unknown_is_none() {
    assert!(try_dispatch_meta("calc").is_none());
    assert_eq!(
        try_dispatch_meta("workflow_execute").map(|m| m.name),
        Some("workflow_execute")
    );
}

#[test]
fn sync_default_inline_tools() {
    let cfg = test_cfg();
    assert!(sync_default_runs_inline(&cfg, "get_current_time"));
    assert!(sync_default_runs_inline(&cfg, "convert_units"));
    assert!(!sync_default_runs_inline(&cfg, "read_file"));
    assert!(!sync_default_runs_inline(&cfg, "calc"));
}

#[test]
fn meta_fields_and_default_class() {
    let wf = try_dispatch_meta("workflow_execute").unwrap();
    assert!(!wf.requires_workspace);
    assert_eq!(wf.class, ToolExecutionClass::Workflow);
    let rc = try_dispatch_meta("run_command").unwrap();
    assert!(rc.requires_workspace);
    assert_eq!(rc.class, ToolExecutionClass::CommandSpawnTimeout);
    assert_eq!(
        execution_class_for_tool("calc"),
        ToolExecutionClass::BlockingSync
    );
}

#[test]
fn parallel_tool_wall_timeout_secs_smoke() {
    let cfg = crate::config::load_config(None).expect("embed default");
    let cmd_budget = parallel_tool_wall_timeout_secs(&cfg, "read_file");
    assert!(cmd_budget >= 1);
    let fetch_budget = parallel_tool_wall_timeout_secs(&cfg, "http_fetch");
    assert!(fetch_budget >= cmd_budget);
    assert_eq!(
        parallel_tool_wall_timeout_secs(&cfg, "get_weather"),
        cfg.weather_timeout_secs.max(1)
    );
}
