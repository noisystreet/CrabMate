//! 从工作区文件加载作者层 YAML/Markdown 并解析为 `workflow_execute` 参数。

use std::path::Path;

use serde_json::Value;

use crate::tools::resolve_workspace_path_for_read;

use super::compile_spec::compile_workflow_author_yaml;
use super::md_extract::extract_first_crabmate_workflow_block;

/// 读取工作区内工作流文件正文（`.yaml` / `.yml` / `.md` 围栏）。
pub fn read_workflow_author_source(workspace: &Path, rel_path: &str) -> Result<String, String> {
    let resolved = resolve_workspace_path_for_read(workspace, rel_path.trim())
        .map_err(|e| format!("错误：{}", e.user_message()))?;
    let text = std::fs::read_to_string(&resolved)
        .map_err(|e| format!("读取工作流文件失败（{}）：{e}", resolved.display()))?;
    let ext = resolved
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

/// 编译工作区内的作者层文件 → `{"workflow":{...}}`。
pub fn compile_workflow_author_file(workspace: &Path, rel_path: &str) -> Result<Value, String> {
    let yaml = read_workflow_author_source(workspace, rel_path)?;
    compile_workflow_author_yaml(&yaml)
}

/// 将 `workflow_file` 解析为内联 `workflow`；保留顶层 `validate_only` 等键。
pub fn resolve_workflow_execute_args(
    args_json: &str,
    workspace: &Path,
    workspace_is_set: bool,
) -> Result<String, String> {
    let mut root: Value = serde_json::from_str(args_json)
        .map_err(|e| format!("workflow_execute 参数 JSON 无效: {e}"))?;

    let workflow_file = root
        .get("workflow_file")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let inline_workflow = root.get("workflow").cloned();

    let Some(path) = workflow_file else {
        if inline_workflow.is_some() {
            return Ok(args_json.to_string());
        }
        return Err(
            "workflow_execute 须提供 workflow 或 workflow_file（工作区相对路径）".to_string(),
        );
    };

    if !workspace_is_set {
        return Err(
            "workflow_file 须先设置工作区（Web 侧栏或 CLI --workspace / 配置 run_command_working_dir）"
                .to_string(),
        );
    }

    let mut compiled = compile_workflow_author_file(workspace, &path)?;
    if let Some(overlay) = inline_workflow {
        merge_workflow_object_overlay(&mut compiled, &overlay);
    }

    let workflow = compiled
        .get("workflow")
        .cloned()
        .ok_or_else(|| "workflow_file 编译结果缺少 workflow 对象".to_string())?;
    if let Some(obj) = root.as_object_mut() {
        obj.remove("workflow_file");
        obj.insert("workflow".to_string(), workflow);
    }

    serde_json::to_string(&root).map_err(|e| format!("workflow_execute 参数序列化失败: {e}"))
}

fn merge_workflow_object_overlay(compiled: &mut Value, overlay: &Value) {
    let Some(overlay_obj) = overlay.as_object() else {
        return;
    };
    let Some(wf) = compiled.get_mut("workflow") else {
        return;
    };
    let Some(wf_obj) = wf.as_object_mut() else {
        return;
    };
    for (k, v) in overlay_obj {
        if matches!(k.as_str(), "nodes" | "steps") {
            continue;
        }
        wf_obj.insert(k.clone(), v.clone());
    }
}
