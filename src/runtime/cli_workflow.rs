//! `crabmate workflow`：YAML / Markdown 作者层编译、校验与执行（不要求 API_KEY）。

use std::path::Path;

use crate::agent::workflow::{
    WorkflowApprovalMode, compile_workflow_author_yaml, extract_first_crabmate_workflow_block,
    parse_workflow_spec_from_json, run_workflow_execute_tool, workflow_topo_layers,
};
use crate::config::AgentConfig;
use crate::config::cli::WorkflowFileCli;
use crate::runtime::cli::cli_effective_work_dir;

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
    let (out, _workspace_changed) = run_workflow_execute_tool(
        &args,
        cfg,
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
