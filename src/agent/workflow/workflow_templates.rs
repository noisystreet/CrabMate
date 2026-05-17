//! 内置 `workflow_execute` 模板：减少手写 DAG JSON，与 `parse_workflow_spec` 合并逻辑。

pub(crate) const SUPPORTED_WORKFLOW_TEMPLATES: &[&str] =
    &["rust_ci_light", "code_review", "refactor_precheck"];

/// `workflow_template: "rust_ci_light"` 时的默认 DAG：`fmt → check → clippy → test`（串行，`cargo_*` 结构化工具）。
pub(crate) fn workflow_template_rust_ci_light() -> serde_json::Value {
    serde_json::json!({
        "max_parallelism": 4,
        "fail_fast": true,
        "compensate_on_failure": true,
        "nodes": [
            {
                "id": "rust_ci_fmt",
                "tool_name": "cargo_fmt_check",
                "tool_args": {},
                "deps": []
            },
            {
                "id": "rust_ci_check",
                "tool_name": "cargo_check",
                "tool_args": { "all_targets": true },
                "deps": ["rust_ci_fmt"]
            },
            {
                "id": "rust_ci_clippy",
                "tool_name": "cargo_clippy",
                "tool_args": { "all_targets": true },
                "deps": ["rust_ci_check"]
            },
            {
                "id": "rust_ci_test",
                "tool_name": "cargo_test",
                "tool_args": {},
                "deps": ["rust_ci_clippy"]
            }
        ]
    })
}

/// `code_review`：变更文件名 → diff → Rust 风险模式检索 → `cargo_clippy`（只读审查，不 `fail_fast`）。
pub(crate) fn workflow_template_code_review() -> serde_json::Value {
    serde_json::json!({
        "max_parallelism": 4,
        "fail_fast": false,
        "compensate_on_failure": false,
        "nodes": [
            {
                "id": "cr_diff_names",
                "tool_name": "git_diff_names",
                "tool_args": { "mode": "all" },
                "deps": [],
                "node_tool_role": "review_readonly"
            },
            {
                "id": "cr_diff",
                "tool_name": "git_diff",
                "tool_args": { "mode": "all", "context_lines": 3 },
                "deps": ["cr_diff_names"],
                "node_tool_role": "review_readonly"
            },
            {
                "id": "cr_search",
                "tool_name": "search_in_files",
                "tool_args": {
                    "pattern": "unwrap\\(|\\.expect\\(|todo!|FIXME|panic!",
                    "file_glob": "*.rs",
                    "max_results": 40,
                    "case_insensitive": true
                },
                "deps": ["cr_diff"],
                "node_tool_role": "review_readonly"
            },
            {
                "id": "cr_clippy",
                "tool_name": "cargo_clippy",
                "tool_args": { "all_targets": true },
                "deps": ["cr_diff", "cr_search"],
                "node_tool_role": "review_readonly"
            }
        ]
    })
}

/// `refactor_precheck`：`call_graph_sketch` → `find_references`（须在 workflow 上提供 `refactor_symbol`）。
pub(crate) fn workflow_template_refactor_precheck() -> serde_json::Value {
    serde_json::json!({
        "max_parallelism": 2,
        "fail_fast": false,
        "compensate_on_failure": false,
        "nodes": [
            {
                "id": "rpc_graph",
                "tool_name": "call_graph_sketch",
                "tool_args": { "symbol": "" },
                "deps": [],
                "node_tool_role": "review_readonly"
            },
            {
                "id": "rpc_refs",
                "tool_name": "find_references",
                "tool_args": { "symbol": "", "exclude_definitions": true },
                "deps": ["rpc_graph"],
                "node_tool_role": "review_readonly"
            }
        ]
    })
}

/// 将 `refactor_symbol` / `symbol` 写入预检模板节点的 `tool_args.symbol`（及 `call_graph_sketch.symbols`）。
pub(crate) fn apply_refactor_symbol_to_workflow(
    workflow: &mut serde_json::Value,
    symbol: &str,
) -> Result<(), String> {
    let sym = symbol.trim();
    if sym.is_empty() {
        return Err("refactor_symbol 不能为空".to_string());
    }
    let nodes = workflow
        .get_mut("nodes")
        .and_then(|x| x.as_array_mut())
        .ok_or("workflow 缺少 nodes 数组")?;
    for node in nodes.iter_mut() {
        let id = node
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if id != "rpc_graph" && id != "rpc_refs" {
            continue;
        }
        let args = node
            .as_object_mut()
            .and_then(|m| m.get_mut("tool_args"))
            .and_then(|x| x.as_object_mut())
            .ok_or_else(|| format!("节点 {id} 缺少 tool_args 对象"))?;
        args.insert(
            "symbol".to_string(),
            serde_json::Value::String(sym.to_string()),
        );
        if id == "rpc_graph" {
            args.insert("symbols".to_string(), serde_json::json!([sym]));
        }
    }
    Ok(())
}

/// 可选覆盖 `code_review` 中 `cr_search` 的检索正则（`review_search_pattern`）。
pub(crate) fn apply_code_review_search_pattern(
    workflow: &mut serde_json::Value,
    pattern: &str,
) -> Result<(), String> {
    let pat = pattern.trim();
    if pat.is_empty() {
        return Err("review_search_pattern 不能为空".to_string());
    }
    let nodes = workflow
        .get_mut("nodes")
        .and_then(|x| x.as_array_mut())
        .ok_or("workflow 缺少 nodes 数组")?;
    for node in nodes.iter_mut() {
        if node.get("id").and_then(|x| x.as_str()) != Some("cr_search") {
            continue;
        }
        let args = node
            .as_object_mut()
            .and_then(|m| m.get_mut("tool_args"))
            .and_then(|x| x.as_object_mut())
            .ok_or("节点 cr_search 缺少 tool_args 对象")?;
        args.insert(
            "pattern".to_string(),
            serde_json::Value::String(pat.to_string()),
        );
        return Ok(());
    }
    Err("code_review 模板缺少节点 cr_search".to_string())
}

/// 将 `workflow` 对象中的键覆盖到模板之上；若含 **`nodes`** 则整体替换节点列表（与手写 DAG 一致）。
pub(crate) fn merge_workflow_template_overlay(
    mut base: serde_json::Value,
    overlay: Option<&serde_json::Value>,
) -> serde_json::Value {
    let Some(over) = overlay else {
        return base;
    };
    let Some(over_map) = over.as_object() else {
        return base;
    };
    if let Some(obj) = base.as_object_mut() {
        for (k, v) in over_map {
            obj.insert(k.clone(), v.clone());
        }
    }
    base
}
