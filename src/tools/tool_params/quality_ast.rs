//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_pre_commit_run() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "hook": { "type": "string", "description": "可选：仅运行指定 hook id（字母数字与 ._-）" },
            "all_files": { "type": "boolean", "description": "可选：是否 --all-files（与 files 同时存在时以 files 为准），默认 false" },
            "files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：--files 相对路径列表；非空时不加 --all-files"
            },
            "verbose": { "type": "boolean", "description": "可选：是否 --verbose，默认 false" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_typos_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：相对工作区根的待检查路径（文件或目录），默认 [\"README.md\",\"docs\"]；仅当路径存在时才会传入 typos，最多 24 项。禁止 .. 与绝对路径。"
            },
            "config_path": {
                "type": "string",
                "description": "可选：typos 配置文件相对路径（如 .typos.toml / typos.toml），可在配置中维护项目词典。"
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_codespell_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：同 typos_check，默认 README.md 与 docs；只读检查，不会写回（封装层不传 -w）。"
            },
            "skip": {
                "type": "string",
                "description": "可选：传给 codespell 的 --skip（如 \"*.svg,*.lock\"），不含换行，最长 512 字符"
            },
            "dictionary_paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：项目词典文件列表（相对路径，逐个传给 codespell -I，最多 8 项）。"
            },
            "ignore_words_list": {
                "type": "string",
                "description": "可选：传给 codespell -L 的逗号分隔忽略词列表（不含换行，最长 512 字符）。"
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_ast_grep_run() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "ast-grep 模式串（如 Rust：`fn $NAME($$$) { $$$ }`）。单行建议；最长 4096 字符。"
            },
            "lang": {
                "type": "string",
                "description": "语言：rust、c/cpp、python、javascript、typescript、tsx、jsx、go、java、kotlin、bash、html、css（可用别名如 rs、py、ts）"
            },
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：搜索根路径（相对工作区），默认 [\"src\"]；仅存在路径会参与；最多 8 项。内置排除 target/node_modules/.git/vendor/dist/build。"
            },
            "globs": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：额外 --globs（如 \"!**/generated/**\"），最多 10 项；禁止 .. 与反引号等。"
            }
        },
        "required": ["pattern", "lang"],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_ast_grep_rewrite() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "ast-grep 模式串，最长 4096 字符"
            },
            "rewrite": {
                "type": "string",
                "description": "替换模板（ast-grep rewrite 模板），最长 4096 字符"
            },
            "lang": {
                "type": "string",
                "description": "语言：rust、c/cpp、python、javascript、typescript、tsx、jsx、go、java、kotlin、bash、html、css"
            },
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：搜索根路径（相对工作区），默认 [\"src\"]；最多 8 项。"
            },
            "globs": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：额外 --globs，最多 10 项（禁止 .. 等非法字符）。"
            },
            "dry_run": {
                "type": "boolean",
                "description": "是否仅预览（默认 true）。false 时会写入文件。"
            },
            "confirm": {
                "type": "boolean",
                "description": "当 dry_run=false 时必须为 true，防止误改。"
            }
        },
        "required": ["pattern", "rewrite", "lang"],
        "additionalProperties": false
    })
}
