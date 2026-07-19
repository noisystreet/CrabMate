//! `crabmate workflow`：YAML / Markdown 作者层编译、校验与执行（不要求 API_KEY）。

use std::path::Path;

use crate::agent::workflow::{
    WorkflowApprovalMode, compile_workflow_author_yaml, extract_first_crabmate_workflow_block,
    parse_workflow_spec_from_json, run_workflow_execute_tool, workflow_topo_layers,
};
use crate::config::cli::WorkflowFileCli;
use crate::config::{AgentConfig, ExposeSecret};
use crate::runtime::cli::cli_effective_work_dir;
use crabmate_workflow::config::WorkflowConfig;

fn load_author_yaml_source(path: &Path) -> Result<String, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "md" || ext == "markdown" {
        extract_first_crabmate_workflow_block(&text)
    } else {
        Ok(text)
    }
}

/// `workflow compile`：将作者层 YAML 编译为 `workflow_execute` 可消费的 JSON 并打印到 stdout。
pub fn run_workflow_compile_command(cli: &WorkflowFileCli) -> Result<(), String> {
    let path = Path::new(&cli.file);
    let yaml = load_author_yaml_source(path)?;
    let compiled = compile_workflow_author_yaml(&yaml)?;
    let out = serde_json::to_string_pretty(&compiled).map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}

/// `workflow validate`：编译 + `parse_workflow_spec` + 拓扑层（等同 validate_only 核心检查）。
pub fn run_workflow_validate_command(cli: &WorkflowFileCli) -> Result<(), String> {
    let path = Path::new(&cli.file);
    let yaml = load_author_yaml_source(path)?;
    let root: serde_json::Value =
        serde_yaml::from_str(&yaml).map_err(|e| format!("workflow_spec YAML 解析失败: {e}"))?;
    let author_mode = crate::agent::workflow::validate_workflow_author_document(&root)?
        .as_str()
        .to_string();
    let compiled = compile_workflow_author_yaml(&yaml)?;
    let args_json =
        serde_json::to_string(&compiled).map_err(|e| format!("JSON 序列化失败: {e}"))?;
    let spec = parse_workflow_spec_from_json(&args_json)?;
    let layers = workflow_topo_layers(&spec.nodes)?;

    if cli.json {
        let payload = serde_json::json!({
            "ok": true,
            "source": path.display().to_string(),
            "author_spec_version": crate::agent::workflow::WORKFLOW_AUTHOR_SPEC_VERSION,
            "author_mode": author_mode,
            "nodes_count": spec.nodes.len(),
            "layer_count": layers.len(),
            "execution_layers": layers,
            "fail_fast": spec.fail_fast,
            "max_parallelism": spec.max_parallelism,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?
        );
    } else {
        println!("workflow validate: OK");
        println!("  source: {}", path.display());
        println!(
            "  author: version {} mode {}",
            crate::agent::workflow::WORKFLOW_AUTHOR_SPEC_VERSION,
            author_mode
        );
        println!("  nodes: {}", spec.nodes.len());
        if !spec.for_each_pending.is_empty() {
            println!("  for_each_pending: {}", spec.for_each_pending.len());
            for p in &spec.for_each_pending {
                println!(
                    "    - {} (from={}, json_path={})",
                    p.base_id,
                    p.from,
                    p.json_path.as_deref().unwrap_or("-")
                );
            }
        }
        println!("  layers: {}", layers.len());
        for (i, layer) in layers.iter().enumerate() {
            println!("  layer {i}: {}", layer.join(", "));
        }
        for n in &spec.nodes {
            let deps = if n.deps.is_empty() {
                String::new()
            } else {
                format!(" deps={}", n.deps.join(","))
            };
            println!("  - {} ({}){deps}", n.id, n.tool_name);
        }
    }
    Ok(())
}

/// `workflow run`：编译并执行工作区内的作者层文件。
pub async fn run_workflow_run_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    cli: WorkflowFileCli,
) -> Result<(), String> {
    let workspace =
        cli_effective_work_dir(workspace_cli, &cfg.command_exec.run_command_working_dir);
    let rel = cli.file.trim();
    if rel.is_empty() {
        return Err("workflow run：文件路径不能为空".to_string());
    }
    let args = serde_json::json!({ "workflow_file": rel }).to_string();
    let wf_cfg = WorkflowConfig {
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
    };
    let (out, _workspace_changed) = run_workflow_execute_tool(
        &args,
        &wf_cfg,
        &workspace,
        true,
        WorkflowApprovalMode::NoApproval,
        cfg.command_exec.command_max_output_len,
        None,
    )
    .await;

    if cli.json {
        println!("{out}");
    } else if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) {
        if let Some(summary) = v.get("human_summary").and_then(|x| x.as_str()) {
            println!("{summary}");
        } else {
            println!("{out}");
        }
    } else {
        println!("{out}");
    }

    let failed = serde_json::from_str::<serde_json::Value>(&out)
        .ok()
        .and_then(|v| {
            v.get("status")
                .and_then(|s| s.as_str())
                .map(|s| s == "failed")
        })
        .unwrap_or(true);
    if failed {
        return Err("workflow run 失败（见上方 human_summary 或 JSON）".to_string());
    }
    Ok(())
}
