//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_find_symbol() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "symbol": { "type":"string", "description":"要定位的符号名（必填）" },
            "path": { "type":"string", "description":"可选：搜索起点子路径（相对工作区根目录，默认 .）" },
            "kind": { "type":"string", "description":"可选：符号类型（fn|struct|enum|trait|const|static|type|mod|any，默认 any）" },
            "max_results": { "type":"integer", "description":"可选：最多返回结果条数（默认 30）", "minimum":1 },
            "context_lines": { "type":"integer", "description":"可选：每条结果输出的上下文行数（默认 2）", "minimum":0 },
            "case_insensitive": { "type":"boolean", "description":"可选：是否忽略大小写（默认 true）" },
            "include_hidden": { "type":"boolean", "description":"可选：是否包含隐藏文件（以 . 开头），默认 false" }
        },
        "required":["symbol"],
        "additionalProperties":false
    })
}

pub(in crate::tools) fn params_find_references() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "symbol": { "type":"string", "description":"要查找引用的标识符名（必填）" },
            "path": { "type":"string", "description":"可选：仅在某子路径下搜索（相对工作区）" },
            "max_results": { "type":"integer", "description":"可选：最多返回条数，默认 80，上限 300", "minimum":1 },
            "case_sensitive": { "type":"boolean", "description":"可选：是否大小写敏感（默认 false，即忽略大小写）" },
            "exclude_definitions": { "type":"boolean", "description":"可选：是否跳过疑似定义行（默认 true）" },
            "include_hidden": { "type":"boolean", "description":"可选：是否遍历隐藏目录（默认 false）" }
        },
        "required":["symbol"],
        "additionalProperties":false
    })
}
