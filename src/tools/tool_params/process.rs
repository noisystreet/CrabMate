//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_port_check() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "port":{"type":"integer","description":"要检查的端口号（1-65535，必填）","minimum":1,"maximum":65535}
        },
        "required":["port"]
    })
}

pub(in crate::tools) fn params_process_list() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "filter":{"type":"string","description":"可选：按进程名/命令行关键词过滤（不区分大小写）"},
            "user_only":{"type":"boolean","description":"是否仅当前用户进程，默认 true"},
            "max_count":{"type":"integer","description":"最多返回条数，默认 100","minimum":1,"maximum":500}
        },
        "required":[]
    })
}

// ── 代码度量与分析 ──────────────────────────────────────────
