//! `workflow_execute` 工具入口：解析参数、validate_only 规划、DAG 执行。

use crate::config::AgentConfig;
use log::{info, warn};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::dag::{topo_layers, validate_dag};
use super::execute::{
    WorkflowApprovalMode, WorkflowToolExecCtx, execute_workflow_dag, truncate_for_summary,
};
use super::parse::parse_workflow_spec;
use super::types::{
    WORKFLOW_RUN_SEQ, WorkflowExecutionCompensationReport, WorkflowExecutionNodeReport,
    WorkflowExecutionReport, WorkflowExecutionStats,
};

pub async fn run_workflow_execute_tool(
    args_json: &str,
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    approval_mode: WorkflowApprovalMode,
    command_max_output_len: usize,
) -> (String, bool) {
    let workflow_run_id = WORKFLOW_RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    info!(
        target: "crabmate",
        "workflow_execute start workflow_run_id={} workspace_is_set={}",
        workflow_run_id,
        workspace_is_set
    );
    // 支持反思阶段的“done=true”：运行时应跳过 DAG 执行，
    // 只返回一个明确的结果，避免模型误触发重复执行。
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => {
            warn!(
                target: "crabmate",
                "workflow_execute args parse failed workflow_run_id={}",
                workflow_run_id
            );
            let report = serde_json::json!({
                "type": "workflow_execute_error",
                "status": "failed",
                "workspace_changed": false,
                "human_summary": "workflow_execute 参数解析错误"
            });
            return (report.to_string(), false);
        }
    };
    let workflow_v = v.get("workflow").unwrap_or(&v);

    let done = workflow_v
        .get("done")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if done {
        info!(
            target: "crabmate",
            "workflow_execute skip by done=true workflow_run_id={}",
            workflow_run_id
        );
        let report = serde_json::json!({
            "type": "workflow_execute_done_skip",
            "status": "passed",
            "workspace_changed": false,
            "spec": workflow_v.clone(),
            "stats": { "passed": 0, "failed": 0, "skipped": 0 },
            "nodes": [],
            "first_failure": null,
            "compensation": { "executed": false, "summary": null },
            "human_summary": "workflow_execute: reflection done=true，跳过 DAG 执行。"
        });
        return (report.to_string(), false);
    }

    let validate_only = workflow_v
        .get("validate_only")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if validate_only {
        info!(
            target: "crabmate",
            "workflow_validate_only start workflow_run_id={}",
            workflow_run_id
        );
        let spec = match parse_workflow_spec(args_json) {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    target: "crabmate",
                    "workflow_validate_only parse failed workflow_run_id={} error={}",
                    workflow_run_id,
                    e
                );
                let report = serde_json::json!({
                    "type": "workflow_validate_error",
                    "status": "failed",
                    "workspace_changed": false,
                    "human_summary": format!("workflow_validate 参数解析错误：{}", e)
                });
                return (report.to_string(), false);
            }
        };

        if let Err(e) = validate_dag(&spec.nodes) {
            warn!(
                target: "crabmate",
                "workflow_validate_only dag validation failed workflow_run_id={} error={}",
                workflow_run_id,
                e
            );
            let report = serde_json::json!({
                "type": "workflow_validate_error",
                "status": "failed",
                "workspace_changed": false,
                "human_summary": format!("workflow_validate DAG 校验失败：{}", e)
            });
            return (report.to_string(), false);
        }

        // 计算静态执行层（拓扑层级），用于“规划”输出
        let execution_layers = match topo_layers(&spec.nodes) {
            Ok(l) => l,
            Err(e) => {
                warn!(
                    target: "crabmate",
                    "workflow_validate_only topo layer failed workflow_run_id={} error={}",
                    workflow_run_id,
                    e
                );
                let report = serde_json::json!({
                    "type": "workflow_validate_error",
                    "status": "failed",
                    "workspace_changed": false,
                    "human_summary": format!("workflow_validate 层级计算失败：{}", e)
                });
                return (report.to_string(), false);
            }
        };

        // 反查每个节点所在 layer index
        let mut layer_idx_by_id: HashMap<String, usize> = HashMap::new();
        for (i, layer) in execution_layers.iter().enumerate() {
            for id in layer.iter() {
                layer_idx_by_id.insert(id.clone(), i);
            }
        }

        let node_reports: Vec<WorkflowExecutionNodeReport> = spec
            .nodes
            .iter()
            .map(|n| WorkflowExecutionNodeReport {
                id: n.id.clone(),
                status: "planned".to_string(),
                tool_name: n.tool_name.clone(),
                deps: n.deps.clone(),
                requires_approval: n.requires_approval,
                timeout_secs: n.timeout_secs,
                compensate_with: n.compensate_with.clone(),
                output_preview: truncate_for_summary(
                    &n.tool_args.to_string(),
                    spec.summary_preview_max_chars,
                ),
                workspace_changed: false,
                exit_code: None,
                error_code: None,
                planned_layer: layer_idx_by_id.get(&n.id).copied(),
                max_retries: n.max_retries,
                attempt: 1,
            })
            .collect();

        let topological_order: Vec<String> = execution_layers
            .iter()
            .flat_map(|layer| layer.iter().cloned())
            .collect();

        let report = WorkflowExecutionReport {
            report_type: "workflow_validate_result".to_string(),
            workflow_run_id,
            status: "planned".to_string(),
            workspace_changed: false,
            spec: serde_json::json!({
                "max_parallelism": spec.max_parallelism,
                "fail_fast": spec.fail_fast,
                "compensate_on_failure": spec.compensate_on_failure,
                "output_inject_max_chars": spec.output_inject_max_chars,
                "nodes_count": spec.nodes.len(),
                "execution_layers": execution_layers,
                "layer_count": spec.cached_layer_count,
                "topological_order": topological_order
            }),
            stats: WorkflowExecutionStats {
                passed: 0,
                failed: 0,
                skipped: 0,
            },
            nodes: node_reports,
            first_failure: None,
            compensation: WorkflowExecutionCompensationReport {
                executed: false,
                summary: None,
            },
            trace: vec![],
            completion_order: topological_order.clone(),
            human_summary: format!(
                "workflow_validate_only: DAG 校验通过，已生成规划（planned nodes={}，layers={}）",
                spec.nodes.len(),
                execution_layers.len()
            ),
        };

        info!(
            target: "crabmate",
            "workflow_validate_only planned workflow_run_id={} nodes_count={} layer_count={}",
            workflow_run_id,
            spec.nodes.len(),
            execution_layers.len()
        );
        let json = serde_json::to_string(&report).unwrap_or_else(|_| report.human_summary.clone());
        return (json, false);
    }

    let spec = match parse_workflow_spec(args_json) {
        Ok(s) => s,
        Err(e) => {
            warn!(
                target: "crabmate",
                "workflow_execute parse spec failed workflow_run_id={} error={}",
                workflow_run_id,
                e
            );
            let report = serde_json::json!({
                "type": "workflow_execute_error",
                "status": "failed",
                "workspace_changed": false,
                "human_summary": format!("workflow_execute 参数解析错误：{}", e)
            });
            return (report.to_string(), false);
        }
    };

    if let Err(e) = validate_dag(&spec.nodes) {
        warn!(
            target: "crabmate",
            "workflow_execute dag validation failed workflow_run_id={} error={}",
            workflow_run_id,
            e
        );
        let report = serde_json::json!({
            "type": "workflow_execute_error",
            "status": "failed",
            "workspace_changed": false,
            "human_summary": format!("workflow_execute workflow 校验失败：{}", e)
        });
        return (report.to_string(), false);
    }

    let approval_mode = approval_mode;
    let workdir = effective_working_dir.to_path_buf();
    let allowed_commands = Arc::clone(&cfg.allowed_commands);
    let weather_timeout_secs = cfg.weather_timeout_secs;
    let command_timeout_secs = cfg.command_timeout_secs;
    let web_search_timeout_secs = cfg.web_search_timeout_secs;
    let web_search_provider = cfg.web_search_provider;
    let web_search_api_key = cfg.web_search_api_key.clone();
    let web_search_max_results = cfg.web_search_max_results;
    let http_fetch_timeout_secs = cfg.http_fetch_timeout_secs;
    let http_fetch_max_response_bytes = cfg.http_fetch_max_response_bytes;
    let http_fetch_allowed_prefixes = cfg.http_fetch_allowed_prefixes.clone();

    let tool_exec_ctx = WorkflowToolExecCtx {
        cfg_command_timeout_secs: command_timeout_secs,
        cfg_weather_timeout_secs: weather_timeout_secs,
        cfg_web_search_timeout_secs: web_search_timeout_secs,
        cfg_web_search_provider: web_search_provider,
        cfg_web_search_api_key: web_search_api_key,
        cfg_web_search_max_results: web_search_max_results,
        cfg_http_fetch_timeout_secs: http_fetch_timeout_secs,
        cfg_http_fetch_max_response_bytes: http_fetch_max_response_bytes,
        cfg_http_fetch_allowed_prefixes: http_fetch_allowed_prefixes,
        cfg_allowed_commands: allowed_commands,
        effective_working_dir: workdir,
        workspace_is_set,
        command_max_output_len,
        test_result_cache_enabled: cfg.test_result_cache_enabled,
        test_result_cache_max_entries: cfg.test_result_cache_max_entries,
        workflow_run_id,
        trace_events: None,
    };

    let (main_result, workspace_changed) =
        execute_workflow_dag(spec, approval_mode, tool_exec_ctx).await;
    info!(
        target: "crabmate",
        "workflow_execute finished workflow_run_id={} workspace_changed={}",
        workflow_run_id,
        workspace_changed
    );
    (main_result, workspace_changed)
}
