//! GitHub CLI（`gh`）封装工具的 JSON Schema。

pub(in crate::tools) fn params_gh_pr_list() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "repo": {
                "type": "string",
                "description": "可选。仓库 `owner/repo`；省略则使用当前目录关联的远程仓库。"
            },
            "state": {
                "type": "string",
                "description": "PR 状态：`open`（默认）、`closed`、`merged`、`all`。",
                "enum": ["open", "closed", "merged", "all"]
            },
            "limit": {
                "type": "integer",
                "description": "最多列出条数，默认 30，上限 200。"
            },
            "fields": {
                "type": "array",
                "items": { "type": "string" },
                "description": "传给 `gh pr list --json` 的字段名列表（如 number、title、author、state）。若提供，成功时工具会在输出末尾附加**格式化后的 JSON** 便于模型消费。"
            },
            "web": {
                "type": "boolean",
                "description": "为 true 时追加 `--web`（在服务器/无头环境通常不可用）。"
            },
            "extra_args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "附加传给 `gh` 的参数（不得含 `..` 或以 `/` 开头）。"
            }
        },
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_gh_pr_view() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "number": {
                "type": "integer",
                "description": "Pull request 编号（正整数）。"
            },
            "repo": {
                "type": "string",
                "description": "可选。`owner/repo`。"
            },
            "fields": {
                "type": "array",
                "items": { "type": "string" },
                "description": "传给 `gh pr view --json` 的字段名；若提供则附加解析后的格式化 JSON。"
            },
            "web": { "type": "boolean", "description": "追加 `--web`。" },
            "extra_args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "附加参数（安全规则同 run_command）。"
            }
        },
        "required": ["number"],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_gh_issue_list() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "repo": { "type": "string", "description": "可选。`owner/repo`。" },
            "state": {
                "type": "string",
                "description": "`open`（默认）、`closed`、`all`。",
                "enum": ["open", "closed", "all"]
            },
            "limit": {
                "type": "integer",
                "description": "列表条数上限，默认 30，最大 200。"
            },
            "fields": {
                "type": "array",
                "items": { "type": "string" },
                "description": "`gh issue list --json` 字段；若提供则附加格式化 JSON。"
            },
            "web": { "type": "boolean" },
            "extra_args": {
                "type": "array",
                "items": { "type": "string" }
            }
        },
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_gh_issue_view() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "number": { "type": "integer", "description": "Issue 编号。" },
            "repo": { "type": "string" },
            "fields": {
                "type": "array",
                "items": { "type": "string" },
                "description": "`gh issue view --json` 字段。"
            },
            "web": { "type": "boolean" },
            "extra_args": {
                "type": "array",
                "items": { "type": "string" }
            }
        },
        "required": ["number"],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_gh_run_list() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "repo": { "type": "string", "description": "可选。`owner/repo`。" },
            "limit": {
                "type": "integer",
                "description": "运行记录条数，默认 30，最大 200。"
            },
            "fields": {
                "type": "array",
                "items": { "type": "string" },
                "description": "`gh run list --json` 字段。"
            },
            "web": { "type": "boolean" },
            "extra_args": {
                "type": "array",
                "items": { "type": "string" }
            }
        },
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_gh_api() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "GitHub API **相对路径**（不含前导 `/`），如 `repos/owner/repo/issues`。仅允许字母数字与 `/_@.-:`，不得含 `..`。"
            },
            "method": {
                "type": "string",
                "description": "HTTP 方法，默认 GET。",
                "enum": ["GET", "HEAD", "POST", "PATCH", "PUT", "DELETE"]
            },
            "body": {
                "type": "string",
                "description": "可选 JSON 请求体字符串（用于 POST/PATCH/PUT）；经 stdin 传给 `gh api`。GET 不得与非空 body 同用。"
            },
            "extra_args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "传给 `gh api` 的额外参数（须通过安全校验）。"
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}
