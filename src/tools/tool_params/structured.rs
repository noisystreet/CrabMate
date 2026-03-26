//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_structured_validate() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作区的 JSON / YAML / TOML / CSV / TSV 文件路径（如 package.json、data.csv）"
            },
            "format": {
                "type": "string",
                "description": "可选：auto（按扩展名推断）或 json / yaml|yml / toml / csv / tsv",
                "enum": ["auto", "json", "yaml", "yml", "toml", "csv", "tsv"]
            },
            "has_header": {
                "type": "boolean",
                "description": "仅 CSV/TSV：首行是否为列名；true 时解析为对象数组，false 时为字符串数组的数组；JSON/YAML/TOML 忽略。默认 true"
            },
            "summarize": {
                "type": "boolean",
                "description": "可选：校验通过后是否输出顶层结构摘要，默认 true"
            }
        },
        "required": ["path"]
    })
}

pub(in crate::tools) fn params_structured_query() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的数据文件路径" },
            "query": {
                "type": "string",
                "description": "路径：以 / 开头为 JSON Pointer（RFC 6901，如 /dependencies/serde）；否则为点号路径（如 dependencies.serde；纯数字段作数组下标）"
            },
            "format": {
                "type": "string",
                "description": "可选：auto / json / yaml|yml / toml / csv / tsv",
                "enum": ["auto", "json", "yaml", "yml", "toml", "csv", "tsv"]
            },
            "has_header": {
                "type": "boolean",
                "description": "仅 CSV/TSV：首行是否为列名；默认 true"
            }
        },
        "required": ["path", "query"]
    })
}

pub(in crate::tools) fn params_structured_diff() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path_a": { "type": "string", "description": "相对工作区的第一份文件（如 openapi.old.json）" },
            "path_b": { "type": "string", "description": "相对工作区的第二份文件（如 openapi.new.json）" },
            "format": {
                "type": "string",
                "description": "可选：对两边使用同一格式；auto 时按各自扩展名分别推断",
                "enum": ["auto", "json", "yaml", "yml", "toml", "csv", "tsv"]
            },
            "has_header": {
                "type": "boolean",
                "description": "仅 CSV/TSV：首行是否为列名；对 path_a 与 path_b 使用同一语义；默认 true"
            },
            "max_diff_lines": {
                "type": "integer",
                "description": "最多输出多少条差异路径，默认 200，上限 2000",
                "minimum": 1,
                "maximum": 2000
            }
        },
        "required": ["path_a", "path_b"]
    })
}

pub(in crate::tools) fn params_structured_patch() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的数据文件路径（json/yaml/yml/toml）" },
            "query": {
                "type": "string",
                "description": "目标路径：JSON Pointer（/a/b）或点号路径（a.b.0）"
            },
            "action": {
                "type": "string",
                "enum": ["set", "remove"],
                "description": "补丁动作，默认 set"
            },
            "value": {
                "description": "仅 action=set 需要：写入值（任意 JSON 值）",
                "oneOf": [
                    {"type":"object"},
                    {"type":"array"},
                    {"type":"string"},
                    {"type":"number"},
                    {"type":"integer"},
                    {"type":"boolean"},
                    {"type":"null"}
                ]
            },
            "format": {
                "type": "string",
                "description": "可选：auto / json / yaml|yml / toml",
                "enum": ["auto", "json", "yaml", "yml", "toml"]
            },
            "create_missing": {
                "type": "boolean",
                "description": "action=set 时中间路径缺失是否自动创建，默认 true"
            },
            "dry_run": {
                "type": "boolean",
                "description": "默认 true：仅预览；false 将实际写入"
            },
            "confirm": {
                "type": "boolean",
                "description": "当 dry_run=false 时必须 true"
            }
        },
        "required": ["path", "query"],
        "additionalProperties": false
    })
}
