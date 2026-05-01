//! 内置 `workflow_execute` 模板：减少手写 DAG JSON，与 `parse_workflow_spec` 合并逻辑。

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
