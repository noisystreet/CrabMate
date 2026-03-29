//! `docs_health_sweep` 聚合工具的 JSON Schema。

pub(in crate::tools) fn params_docs_health_sweep() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "run_doc_preview": { "type": "boolean", "description": "是否预览主文档前几行，默认 true" },
            "doc_paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "预览用的相对路径列表；默认与 repo_overview_sweep 主文档列表一致"
            },
            "doc_preview_max_lines": {
                "type": "integer",
                "description": "每个预览文件最多行数，默认 60，范围 10～200",
                "minimum": 10,
                "maximum": 200
            },
            "run_typos": { "type": "boolean", "description": "是否运行 typos_check，默认 true" },
            "run_codespell": { "type": "boolean", "description": "是否运行 codespell_check，默认 true" },
            "run_markdown_links": { "type": "boolean", "description": "是否运行 markdown_check_links，默认 true" },
            "spell_paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：同时传给 typos 与 codespell 的 paths（相对路径）；未设则二者各自默认 README+docs"
            },
            "typos_config_path": { "type": "string", "description": "可选：typos 配置相对路径（如 .typos.toml）" },
            "codespell_skip": { "type": "string", "description": "可选：codespell --skip" },
            "codespell_dictionary_paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：codespell -I 词典文件相对路径列表"
            },
            "codespell_ignore_words_list": { "type": "string", "description": "可选：codespell -L" },
            "md_roots": {
                "type": "array",
                "items": { "type": "string" },
                "description": "markdown_check_links 的 roots；默认单工具行为（README.md + docs）"
            },
            "md_max_files": { "type": "integer", "description": "markdown_check_links max_files" },
            "md_max_depth": { "type": "integer", "description": "markdown_check_links max_depth" },
            "md_allowed_external_prefixes": {
                "type": "array",
                "items": { "type": "string" },
                "description": "非空时对外链发 HEAD（内置 HTTP，不经 http_fetch 白名单/审批）；空则只检相对链接与锚点"
            },
            "md_external_timeout_secs": { "type": "integer", "description": "外链探测超时秒数" },
            "md_check_fragments": { "type": "boolean", "description": "是否校验 #fragment 锚点" },
            "md_output_format": {
                "type": "string",
                "description": "markdown_check_links 输出：text / json / sarif",
                "enum": ["text", "json", "sarif"]
            },
            "fail_fast": { "type": "boolean", "description": "遇 typos/codespell 非零退出时是否跳过后续步骤，默认 false" },
            "summary_only": { "type": "boolean", "description": "仅输出步骤汇总，默认 false" }
        },
        "required": [],
        "additionalProperties": false
    })
}
