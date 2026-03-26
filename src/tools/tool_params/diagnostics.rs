//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_changelog_draft() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "since": {
                "type": "string",
                "description": "可选：范围起点（tag/提交/分支）；与 until 组成 since..until"
            },
            "until": {
                "type": "string",
                "description": "可选：范围终点；默认与 HEAD 组合见 since；都空则从 HEAD 回溯"
            },
            "max_commits": {
                "type": "integer",
                "description": "最多纳入多少条提交，默认 500，上限 2000",
                "minimum": 1,
                "maximum": 2000
            },
            "group_by": {
                "type": "string",
                "description": "聚合方式：date=按提交日；flat=平铺列表；tag_ranges 或 tags=按相邻 tag 区间（semver 降序，需至少 2 个 tag）",
                "enum": ["date", "flat", "tag_ranges", "tags"]
            },
            "max_tag_sections": {
                "type": "integer",
                "description": "tag_ranges 时最多几段区间（每段一对相邻 tag），默认 25，上限 100",
                "minimum": 1,
                "maximum": 100
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_license_notice() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "workspace_only": {
                "type": "boolean",
                "description": "仅列出工作区成员包（默认 false：含解析图中的传递依赖）"
            },
            "max_crates": {
                "type": "integer",
                "description": "表格最多多少行（按 crate 名去重后），默认 500，上限 3000",
                "minimum": 1,
                "maximum": 3000
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_diagnostic_summary() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "include_toolchain": {
                "type": "boolean",
                "description": "是否输出 rustc/cargo/rustup/bc 与 OS 架构，默认 true"
            },
            "include_workspace_paths": {
                "type": "boolean",
                "description": "是否检查工作区 target/、Cargo.toml、frontend 等路径，默认 true"
            },
            "include_env": {
                "type": "boolean",
                "description": "是否列出关键环境变量仅状态（永不输出取值），默认 true"
            },
            "extra_env_vars": {
                "type": "array",
                "items": { "type": "string" },
                "description": "额外变量名，须为大写 [A-Z0-9_]+（如 CI）；与内置列表合并且去重"
            }
        },
        "required": []
    })
}
