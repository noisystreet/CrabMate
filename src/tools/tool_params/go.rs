//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_go_build() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：go build 包路径或模式，默认 ./...；禁止 .. 与绝对路径" },
            "output": { "type": "string", "description": "可选：-o 输出可执行文件相对路径；禁止 .. 与绝对路径" },
            "verbose": { "type": "boolean", "description": "可选：是否 -v，默认 false" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_go_test() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：测试包路径，默认 ./...；禁止 .. 与绝对路径" },
            "run": { "type": "string", "description": "可选：-run 测试名过滤（保守字符集）" },
            "verbose": { "type": "boolean", "description": "可选：是否 -v，默认 false" },
            "short": { "type": "boolean", "description": "可选：是否 -short，默认 false" },
            "count": { "type": "integer", "description": "可选：-count，须为正整数", "minimum": 1 },
            "timeout": { "type": "string", "description": "可选：-timeout，如 30s（短字符串、无空白）" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_go_vet() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：go vet 包路径，默认 ./...；禁止 .. 与绝对路径" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_go_mod_tidy() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "verbose": { "type": "boolean", "description": "可选：是否 -v，默认 false" },
            "confirm": { "type": "boolean", "description": "须为 true 才会执行（写回 go.mod/go.sum）" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_go_fmt_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：传给 gofmt -l 的相对路径列表，默认 [\".\"]；禁止 .. 与绝对路径"
            }
        },
        "required": []
    })
}
