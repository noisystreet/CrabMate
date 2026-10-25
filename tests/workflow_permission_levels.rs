#![cfg(feature = "server")]
//! workflow 节点层的越界策略（设计 §7.2 / §13）：L2 直接拒、L4 放行，**都不弹窗**。
//!
//! 「不弹窗」用 `tokio::time::timeout` 兜底断言：这里的审批通道永远收不到决策，
//! 所以只要实现回退到人工审批，节点就会挂到超时 —— 测试随即失败。

use std::sync::Arc;
use std::time::Duration;

use crabmate::agent::workflow::config::WorkflowConfig;
use crabmate::agent::workflow::{WorkflowApprovalMode, run_workflow_execute_tool};
use crabmate::cm_agent::permission::{ApprovalPolicy, CommandScope, PermissionCaps, project_caps};
use crabmate::cm_types::{CommandApprovalDecision, PermissionLevel};
use crabmate::config::{AgentConfig, ExposeSecret};
use crabmate::load_config;
use tokio::sync::{Mutex, mpsc};

/// 一个**永远不会有决策**的审批通道：只用来证明节点没有去问人。
struct ApprovalChannel {
    out_tx: mpsc::Sender<String>,
    approval_rx: Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>>,
    persistent: Arc<Mutex<std::collections::HashSet<String>>>,
    /// 两端都要活着，否则 `request_approval` 会走「通道不可用」而不是「等决策」。
    _keep_out_rx: mpsc::Receiver<String>,
    _keep_decision_tx: mpsc::Sender<CommandApprovalDecision>,
}

impl ApprovalChannel {
    fn new() -> Self {
        let (out_tx, out_rx) = mpsc::channel(8);
        let (decision_tx, decision_rx) = mpsc::channel(8);
        Self {
            out_tx,
            approval_rx: Arc::new(Mutex::new(decision_rx)),
            persistent: Arc::new(Mutex::new(std::collections::HashSet::new())),
            _keep_out_rx: out_rx,
            _keep_decision_tx: decision_tx,
        }
    }

    fn mode(&self, approval: ApprovalPolicy, level: PermissionLevel) -> WorkflowApprovalMode {
        WorkflowApprovalMode::Interactive {
            out_tx: self.out_tx.clone(),
            approval_rx: Arc::clone(&self.approval_rx),
            approval_request_guard: Arc::new(Mutex::new(())),
            persistent_allowlist: Arc::clone(&self.persistent),
            cancel: None,
            permission_level: Some(level),
            approval: Some(approval),
        }
    }
}

fn narrow_cfg(allowed: &[&str]) -> AgentConfig {
    let mut cfg = load_config(None).expect("default config");
    cfg.command_exec.allowed_commands = allowed
        .iter()
        .map(|s| (*s).to_string())
        .collect::<Vec<_>>()
        .into();
    cfg
}

fn node_report(json: &str, id: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(json).expect("report json");
    v.get("nodes")
        .and_then(|x| x.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|n| n.get("id").and_then(|x| x.as_str()) == Some(id))
        })
        .cloned()
        .unwrap_or_else(|| panic!("node `{id}` not found in report: {json}"))
}

/// 节点报告里正文的键名是 `output_preview`（`output` 只出现在部分变体里）。
fn output_of(node: &serde_json::Value) -> String {
    for key in ["output", "output_preview"] {
        if let Some(text) = node.get(key).and_then(|x| x.as_str()) {
            return text.to_string();
        }
    }
    String::new()
}

const ECHO_NODE: &str = r#"{"workflow":{"max_parallelism":1,"nodes":[
    {"id":"probe","tool_name":"run_command","tool_args":{"command":"echo","args":["scope-probe"]},"deps":[]}
]}}"#;

/// L2 = 工作区硬边界：白名单外命令直接拒，**不得弹审批**。
#[tokio::test]
async fn workspace_level_denies_command_node_without_prompting() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = narrow_cfg(&["ls"]);
    let wf_cfg = workflow_config_from_cfg(&cfg);
    let l2: PermissionCaps = project_caps(PermissionLevel::Workspace, &cfg);
    assert_eq!(l2.approval, ApprovalPolicy::Deny, "L2 必须不给人工审批");

    let ch = ApprovalChannel::new();
    let (json, workspace_changed) = tokio::time::timeout(
        Duration::from_secs(10),
        run_workflow_execute_tool(
            ECHO_NODE,
            &wf_cfg,
            tmp.path(),
            true,
            ch.mode(ApprovalPolicy::Deny, PermissionLevel::Workspace),
            8192,
            None,
            Some(&l2),
        ),
    )
    .await
    .expect("L2 不该等审批：走到人工审批会挂到超时");

    assert!(!workspace_changed);
    let node = node_report(&json, "probe");
    assert_eq!(node["status"], "failed", "{node}");
    assert_eq!(node["error_code"], "command_not_allowed", "{node}");
    assert!(
        output_of(&node).contains("不在允许列表"),
        "应报白名单外而非审批相关错误：{node}"
    );
}

/// L4 = 不受命令白名单限制：`command_scope = Any` 落成 `["*"]`，节点直接跑，不弹审批。
#[tokio::test]
async fn unrestricted_level_runs_out_of_list_command_without_prompting() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = narrow_cfg(&["ls"]);
    let wf_cfg = workflow_config_from_cfg(&cfg);
    let l4: PermissionCaps = project_caps(PermissionLevel::Unrestricted, &cfg);
    assert!(
        matches!(l4.command_scope, CommandScope::Any),
        "L4 命令面应为 Any：{:?}",
        l4.command_scope
    );

    let ch = ApprovalChannel::new();
    let (json, _workspace_changed) = tokio::time::timeout(
        Duration::from_secs(30),
        run_workflow_execute_tool(
            ECHO_NODE,
            &wf_cfg,
            tmp.path(),
            true,
            ch.mode(ApprovalPolicy::AuditOnly, PermissionLevel::Unrestricted),
            8192,
            None,
            Some(&l4),
        ),
    )
    .await
    .expect("L4 不该等审批：命令面已放宽");

    let node = node_report(&json, "probe");
    assert_eq!(node["status"], "passed", "{node}");
    assert!(
        output_of(&node).contains("scope-probe"),
        "命令应真的执行：{node}"
    );
}

/// L2 下 `requires_approval` 的通用节点也不能弹窗：直接拒（`approval_required`）。
#[tokio::test]
async fn workspace_level_denies_gated_node_without_prompting() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = narrow_cfg(&["ls"]);
    let wf_cfg = workflow_config_from_cfg(&cfg);
    let l2: PermissionCaps = project_caps(PermissionLevel::Workspace, &cfg);

    let args = r#"{"workflow":{"max_parallelism":1,"nodes":[
        {"id":"gated","tool_name":"json_format","tool_args":{"mode":"compact","text":"{\"a\":1}"},"requires_approval":true,"deps":[]}
    ]}}"#;

    let ch = ApprovalChannel::new();
    let (json, _workspace_changed) = tokio::time::timeout(
        Duration::from_secs(10),
        run_workflow_execute_tool(
            args,
            &wf_cfg,
            tmp.path(),
            true,
            ch.mode(ApprovalPolicy::Deny, PermissionLevel::Workspace),
            8192,
            None,
            Some(&l2),
        ),
    )
    .await
    .expect("L2 不该等审批：requires_approval 节点应直接拒");

    let node = node_report(&json, "gated");
    assert_eq!(node["status"], "failed", "{node}");
    assert_eq!(node["error_code"], "approval_required", "{node}");
    assert!(
        output_of(&node).contains("当前权限等级不提供人工审批"),
        "应说明是等级所限而非「未启用通道」：{node}"
    );
}

fn workflow_config_from_cfg(cfg: &AgentConfig) -> WorkflowConfig {
    WorkflowConfig {
        command_timeout_secs: cfg.command_exec.command_timeout_secs,
        weather_timeout_secs: cfg.weather_tool.weather_timeout_secs,
        web_search_timeout_secs: cfg.web_search.web_search_timeout_secs,
        web_search_provider: cfg.web_search.web_search_provider.as_str().to_string(),
        web_search_api_key: cfg
            .web_search
            .web_search_api_key
            .expose_secret()
            .to_string(),
        web_search_max_results: cfg.web_search.web_search_max_results,
        web_search_proxy: cfg.web_search.web_search_proxy.clone(),
        http_fetch_timeout_secs: cfg.http_fetch.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: cfg.http_fetch.http_fetch_max_response_bytes,
        http_fetch_allowed_prefixes: cfg.http_fetch.http_fetch_allowed_prefixes.clone(),
        allowed_commands: cfg.command_exec.allowed_commands.to_vec(),
        command_max_output_len: cfg.command_exec.command_max_output_len,
        test_result_cache_enabled: cfg.chat_queues_cache.test_result_cache_enabled,
        test_result_cache_max_entries: cfg.chat_queues_cache.test_result_cache_max_entries,
        codebase_semantic_enabled: cfg.codebase_semantic.codebase_semantic_search_enabled,
        codebase_semantic_invalidate_on_workspace_change: cfg
            .codebase_semantic
            .codebase_semantic_invalidate_on_workspace_change,
        codebase_semantic_index_sqlite_path: cfg
            .codebase_semantic
            .codebase_semantic_index_sqlite_path
            .clone(),
        codebase_semantic_max_file_bytes: cfg.codebase_semantic.codebase_semantic_max_file_bytes,
        codebase_semantic_chunk_max_chars: cfg.codebase_semantic.codebase_semantic_chunk_max_chars,
        codebase_semantic_top_k: cfg.codebase_semantic.codebase_semantic_top_k,
        codebase_semantic_query_max_chunks: cfg
            .codebase_semantic
            .codebase_semantic_query_max_chunks,
        codebase_semantic_rebuild_max_files: cfg
            .codebase_semantic
            .codebase_semantic_rebuild_max_files,
        codebase_semantic_rebuild_incremental: cfg
            .codebase_semantic
            .codebase_semantic_rebuild_incremental,
        codebase_semantic_hybrid_alpha: cfg.codebase_semantic.codebase_semantic_hybrid_alpha,
        codebase_semantic_fts_top_n: cfg.codebase_semantic.codebase_semantic_fts_top_n,
        codebase_semantic_hybrid_semantic_pool: cfg
            .codebase_semantic
            .codebase_semantic_hybrid_semantic_pool,
    }
}
