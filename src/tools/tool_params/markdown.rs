//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_markdown_check_links() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "roots": {
                "type": "array",
                "items": { "type": "string" },
                "description": "要扫描的相对路径（文件须为 .md，目录则递归收集 .md）。默认 [\"README.md\",\"docs\"]"
            },
            "max_files": {
                "type": "integer",
                "description": "最多处理多少个 Markdown 文件，默认 300，上限 3000",
                "minimum": 1,
                "maximum": 3000
            },
            "max_depth": {
                "type": "integer",
                "description": "目录递归深度上限，默认 24，上限 80",
                "minimum": 1,
                "maximum": 80
            },
            "allowed_external_prefixes": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：仅对这些前缀匹配的 http(s) 或 // 外链发起 HEAD 探测；为空则所有外链仅计数、不联网"
            },
            "external_timeout_secs": {
                "type": "integer",
                "description": "外链探测超时（秒），默认 10，上限 60",
                "minimum": 1,
                "maximum": 60
            },
            "check_fragments": {
                "type": "boolean",
                "description": "是否校验 Markdown 锚点（#fragment），默认 true。"
            },
            "output_format": {
                "type": "string",
                "description": "输出格式：text（默认）/ json / sarif",
                "enum": ["text", "json", "sarif"]
            }
        },
        "required": []
    })
}
