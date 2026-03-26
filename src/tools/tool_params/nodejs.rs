//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_npm_install() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "subdir":{"type":"string","description":"前端子目录（默认 .），如 frontend"},
            "ci":{"type":"boolean","description":"使用 npm ci（默认 false）"},
            "production":{"type":"boolean","description":"仅安装生产依赖，默认 false"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_npm_run() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "script":{"type":"string","description":"npm script 名（必填）"},
            "subdir":{"type":"string","description":"前端子目录（默认 .），如 frontend"},
            "args":{"type":"array","items":{"type":"string"},"description":"传递给 script 的额外参数（-- 之后）"}
        },
        "required":["script"]
    })
}

pub(in crate::tools) fn params_npx_run() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "package":{"type":"string","description":"npx 要执行的包名（必填），如 prettier、eslint"},
            "subdir":{"type":"string","description":"工作子目录（默认 .）"},
            "args":{"type":"array","items":{"type":"string"},"description":"传递给包命令的参数"}
        },
        "required":["package"]
    })
}

pub(in crate::tools) fn params_tsc_check() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "subdir":{"type":"string","description":"前端子目录（默认 .），如 frontend"},
            "project":{"type":"string","description":"可选：tsconfig 路径（-p），默认使用 -b"},
            "strict":{"type":"boolean","description":"是否 --strict，默认 false"}
        },
        "required":[]
    })
}

// ── Go 补充：golangci-lint ──────────────────────────────────
