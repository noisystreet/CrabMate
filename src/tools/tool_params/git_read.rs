//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_git_status() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "porcelain": {
                "type": "boolean",
                "description": "可选：是否使用机器可读的 --porcelain 输出，默认 false"
            },
            "include_untracked": {
                "type": "boolean",
                "description": "可选：是否显示未跟踪文件，默认 true"
            },
            "branch": {
                "type": "boolean",
                "description": "可选：是否显示分支信息，默认 true"
            }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_git_clean_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}

pub(in crate::tools) fn params_git_diff() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "mode": {
                "type": "string",
                "description": "diff 模式：working（未暂存）、staged（已暂存）、all（两者都看）。默认 working。",
                "enum": ["working", "staged", "all"]
            },
            "path": {
                "type": "string",
                "description": "可选：仅查看某个相对路径（文件或目录）的 diff，如 src/main.rs"
            },
            "context_lines": {
                "type": "integer",
                "description": "可选：每处变更展示上下文行数（-U），默认 3",
                "minimum": 0
            }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_git_diff_stat() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "mode":{
                "type":"string",
                "description":"diff 模式：working（未暂存）、staged（已暂存）、all（两者都看）。默认 working。",
                "enum":["working","staged","all"]
            },
            "path":{
                "type":"string",
                "description":"可选：仅查看某个相对路径（文件或目录）的 diff（例如 src/main.rs）"
            }
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_diff_names() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "mode":{
                "type":"string",
                "description":"diff 模式：working（未暂存）、staged（已暂存）、all（两者都看）。默认 working。",
                "enum":["working","staged","all"]
            },
            "path":{
                "type":"string",
                "description":"可选：仅查看某个相对路径（文件或目录）的变更文件名（例如 src/main.rs）"
            }
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_log() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "max_count":{"type":"integer","description":"可选：最多返回提交条数，默认 20","minimum":1},
            "oneline":{"type":"boolean","description":"可选：是否使用单行展示，默认 true"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_show() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "rev":{"type":"string","description":"可选：提交号/引用，默认 HEAD"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_diff_base() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "base":{"type":"string","description":"可选：基准分支，默认 main（对比 base...HEAD）"},
            "context_lines":{"type":"integer","description":"可选：上下文行数，默认 3","minimum":0}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_blame() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"相对路径（必填）"},
            "start_line":{"type":"integer","description":"可选：起始行（需和 end_line 一起使用）","minimum":1},
            "end_line":{"type":"integer","description":"可选：结束行（需和 start_line 一起使用）","minimum":1}
        },
        "required":["path"]
    })
}

pub(in crate::tools) fn params_git_file_history() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"相对路径（必填）"},
            "max_count":{"type":"integer","description":"可选：最多返回提交条数，默认 30","minimum":1}
        },
        "required":["path"]
    })
}

pub(in crate::tools) fn params_git_branch_list() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "include_remote":{"type":"boolean","description":"可选：是否包含远程分支，默认 true"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_empty_object() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{},
        "required":[]
    })
}
