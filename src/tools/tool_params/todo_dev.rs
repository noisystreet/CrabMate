//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_todo_scan() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {"type": "array", "items": {"type": "string"}, "description": "扫描路径（相对工作区，默认[\".\"]）"},
            "markers": {"type": "array", "items": {"type": "string"}, "description": "标记列表（默认 TODO/FIXME/HACK/XXX）"},
            "exclude": {"type": "array", "items": {"type": "string"}, "description": "排除目录名（默认 target/node_modules/.git/vendor/dist/build）"}
        },
        "required": []
    })
}

// ── 源码分析工具参数 ──────────────────────────────────────────
