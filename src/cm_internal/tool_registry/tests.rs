use crate::cm_types::{FunctionCall, ToolCall};

use super::meta::{HandlerId, ToolExecutionClass, execution_class_for_tool, try_dispatch_meta};
use super::policy::{
    parallel_tool_wall_timeout_secs, sync_default_runs_inline,
    tool_calls_allow_parallel_sync_batch, web_search_outer_wall_secs,
};
use super::{
    DispatchToolCall, DispatchToolMemory, DispatchToolObs, DispatchToolParams,
    DispatchToolPolicy, DispatchToolWorkspace, dispatch_tool,
};
use super::runtime::ToolRuntime;

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

fn test_cfg() -> crate::cm_config::AgentConfig {
    crate::cm_config::load_config(None).expect("embed default")
}

fn default_lookup() -> super::HandlerLookupTable {
    super::HandlerLookupTable::default_dispatch()
}

#[test]
fn parallel_sync_batch_two_readonly_sync_tools() {
    let cfg = test_cfg();
    let batch = vec![tc("read_file"), tc("list_dir")];
    assert!(tool_calls_allow_parallel_sync_batch(
        &default_lookup(),
        &cfg,
        &batch
    ));
}

#[test]
fn parallel_sync_batch_mixed_readonly_http_and_search() {
    let cfg = test_cfg();
    assert!(tool_calls_allow_parallel_sync_batch(
        &default_lookup(),
        &cfg,
        &[tc("read_file"), tc("http_fetch")]
    ));
    assert!(tool_calls_allow_parallel_sync_batch(
        &default_lookup(),
        &cfg,
        &[tc("get_weather"), tc("web_search")]
    ));
}

#[test]
fn parallel_sync_batch_denied_for_cargo_or_workflow() {
    let cfg = test_cfg();
    assert!(!tool_calls_allow_parallel_sync_batch(
        &default_lookup(),
        &cfg,
        &[tc("read_file"), tc("cargo_check")]
    ));
    assert!(!tool_calls_allow_parallel_sync_batch(
        &default_lookup(),
        &cfg,
        &[tc("workflow_execute"), tc("read_file")]
    ));
}

#[test]
fn parallel_sync_batch_denied_for_http_request() {
    let cfg = test_cfg();
    assert!(!tool_calls_allow_parallel_sync_batch(
        &default_lookup(),
        &cfg,
        &[tc("read_file"), tc("http_request")]
    ));
}

#[test]
fn parallel_sync_batch_single_tool_false() {
    let cfg = test_cfg();
    assert!(!tool_calls_allow_parallel_sync_batch(
        &default_lookup(),
        &cfg,
        &[tc("read_file")]
    ));
}

#[test]
fn handler_map_resolves_known_tools() {
    let table = default_lookup();
    assert_eq!(table.id_for("workflow_execute"), HandlerId::Workflow);
    assert_eq!(table.id_for("run_command"), HandlerId::RunCommand);
    assert_eq!(table.id_for("terminal_session"), HandlerId::TerminalSession);
    assert_eq!(table.id_for("web_search"), HandlerId::WebSearch);
    assert_eq!(table.id_for("http_request"), HandlerId::HttpRequest);
    assert_eq!(table.id_for("unknown_xyz"), HandlerId::SyncDefault);
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
    let ts = try_dispatch_meta("terminal_session").unwrap();
    assert!(ts.requires_workspace);
    assert_eq!(ts.class, ToolExecutionClass::CommandSpawnTimeout);
    assert_eq!(
        execution_class_for_tool("calc"),
        ToolExecutionClass::BlockingSync
    );
}

#[test]
fn parallel_tool_wall_timeout_secs_smoke() {
    let cfg = crate::cm_config::load_config(None).expect("embed default");
    let cmd_budget = parallel_tool_wall_timeout_secs(&cfg, "read_file");
    assert!(cmd_budget >= 1);
    let fetch_budget = parallel_tool_wall_timeout_secs(&cfg, "http_fetch");
    assert!(fetch_budget >= cmd_budget);
    assert_eq!(
        parallel_tool_wall_timeout_secs(&cfg, "get_weather"),
        cfg.weather_tool.weather_timeout_secs.max(1)
    );
    // 默认 worbrow：外圈 = 内层超时 + 浏览器收尾宽限
    assert_eq!(
        parallel_tool_wall_timeout_secs(&cfg, "web_search"),
        web_search_outer_wall_secs(&cfg)
    );
    assert!(
        web_search_outer_wall_secs(&cfg) > cfg.web_search.web_search_timeout_secs.max(1),
        "outer wall must exceed inner search timeout for teardown headroom"
    );
}

#[tokio::test]
async fn dispatch_tool_single_call_when_retry_disabled() {
    // 默认配置（tool_retry_enabled=false）：不进重试循环，单次成功返回时间文本。
    let cfg = std::sync::Arc::new(test_cfg());
    let mut workspace_changed = false;
    let runtime = ToolRuntime {
        workspace_changed: &mut workspace_changed,
        ctx: None,
    };
    let tc = ToolCall {
        id: "call_t".into(),
        typ: "function".into(),
        function: FunctionCall {
            name: "get_current_time".into(),
            arguments: "{}".into(),
        },
    };
    let lookup = default_lookup();
    let sandbox = crate::cm_internal::tool_sandbox::default_sync_default_sandbox_backend();
    let (out, payload) = dispatch_tool(DispatchToolParams {
        runtime,
        call: DispatchToolCall {
            name: "get_current_time",
            args: "{}",
            tc: &tc,
        },
        workspace: DispatchToolWorkspace {
            effective_working_dir: std::path::Path::new("."),
            workspace_is_set: true,
            workspace_changelist: None,
        },
        policy: DispatchToolPolicy {
            cfg: &cfg,
            turn_allow: None,
            handler_lookup: &lookup,
            sync_default_sandbox_backend: &sandbox,
        },
        obs: DispatchToolObs {
            sse_out_tx: None,
            sse_control_mirror: None,
            cancel: None,
            tool_jobs: None,
        },
        memory: DispatchToolMemory {
            read_file_turn_cache: None,
            long_term_memory: None,
            long_term_memory_scope_id: None,
            mcp_turn: None,
        },
    })
    .await;
    assert!(out.contains("当前时间"), "unexpected: {out}");
    assert!(payload.is_none());
}

#[tokio::test]
async fn dispatch_tool_retry_enabled_success_still_single_call() {
    // 开启重试但工具成功：不触发重试，结果与关闭时一致。
    let mut cfg = test_cfg();
    cfg.tool_registry_policy.tool_registry_tool_retry_enabled = true;
    let cfg = std::sync::Arc::new(cfg);
    let mut workspace_changed = false;
    let runtime = ToolRuntime {
        workspace_changed: &mut workspace_changed,
        ctx: None,
    };
    let tc = ToolCall {
        id: "call_t".into(),
        typ: "function".into(),
        function: FunctionCall {
            name: "get_current_time".into(),
            arguments: "{}".into(),
        },
    };
    let lookup = default_lookup();
    let sandbox = crate::cm_internal::tool_sandbox::default_sync_default_sandbox_backend();
    let (out, _) = dispatch_tool(DispatchToolParams {
        runtime,
        call: DispatchToolCall {
            name: "get_current_time",
            args: "{}",
            tc: &tc,
        },
        workspace: DispatchToolWorkspace {
            effective_working_dir: std::path::Path::new("."),
            workspace_is_set: true,
            workspace_changelist: None,
        },
        policy: DispatchToolPolicy {
            cfg: &cfg,
            turn_allow: None,
            handler_lookup: &lookup,
            sync_default_sandbox_backend: &sandbox,
        },
        obs: DispatchToolObs {
            sse_out_tx: None,
            sse_control_mirror: None,
            cancel: None,
            tool_jobs: None,
        },
        memory: DispatchToolMemory {
            read_file_turn_cache: None,
            long_term_memory: None,
            long_term_memory_scope_id: None,
            mcp_turn: None,
        },
    })
    .await;
    assert!(out.contains("当前时间"), "unexpected: {out}");
}

#[tokio::test]
async fn dispatch_tool_retry_skips_http_fetch_requiring_approval() {
    // 开启重试但 http_fetch URL 未匹配前缀（需审批）：资格门排除，单次调用返回前缀错误（无审批重提示风险）。
    let mut cfg = test_cfg();
    cfg.tool_registry_policy.tool_registry_tool_retry_enabled = true;
    cfg.http_fetch.http_fetch_allowed_prefixes =
        vec!["https://doc.rust-lang.org/".to_string()];
    let cfg = std::sync::Arc::new(cfg);
    let mut workspace_changed = false;
    let runtime = ToolRuntime {
        workspace_changed: &mut workspace_changed,
        ctx: None,
    };
    let tc = ToolCall {
        id: "call_h".into(),
        typ: "function".into(),
        function: FunctionCall {
            name: "http_fetch".into(),
            arguments: r#"{"url":"https://example.com/x"}"#.into(),
        },
    };
    let lookup = default_lookup();
    let sandbox = crate::cm_internal::tool_sandbox::default_sync_default_sandbox_backend();
    let (out, _) = dispatch_tool(DispatchToolParams {
        runtime,
        call: DispatchToolCall {
            name: "http_fetch",
            args: r#"{"url":"https://example.com/x"}"#,
            tc: &tc,
        },
        workspace: DispatchToolWorkspace {
            effective_working_dir: std::path::Path::new("."),
            workspace_is_set: true,
            workspace_changelist: None,
        },
        policy: DispatchToolPolicy {
            cfg: &cfg,
            turn_allow: None,
            handler_lookup: &lookup,
            sync_default_sandbox_backend: &sandbox,
        },
        obs: DispatchToolObs {
            sse_out_tx: None,
            sse_control_mirror: None,
            cancel: None,
            tool_jobs: None,
        },
        memory: DispatchToolMemory {
            read_file_turn_cache: None,
            long_term_memory: None,
            long_term_memory_scope_id: None,
            mcp_turn: None,
        },
    })
    .await;
    assert!(
        out.contains("http_fetch_allowed_prefixes"),
        "unexpected: {out}"
    );
}
