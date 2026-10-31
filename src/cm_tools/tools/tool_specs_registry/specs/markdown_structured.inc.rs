[
ToolSpec {
            name: tool_names::MARKDOWN_CHECK_LINKS,
            description: "扫描工作区内 Markdown（默认 README.md 与 docs/）：校验**相对路径**链接目标是否存在，并可校验 `#fragment` 是否命中目标 Markdown 标题锚点。支持 `output_format=text|json|sarif`。`http(s)://` 与 `//` 外链默认**不联网**；仅当提供 `allowed_external_prefixes` 时对匹配前缀做 HEAD 探测（失败时 GET Range 回退，且同 URL 去重缓存）。`mailto:`/`tel:` 等跳过。",
            category: ToolCategory::Development,
            tags: &[dev_tag::GENERAL],
            parameters: schema_of::<args::MarkdownCheckLinksArgs>,
            runner: ToolRunner::Typed(runner_markdown_check_links_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_markdown_check_links),
        },
        builtin_tool! {
            name: tool_names::STRUCTURED,
            args: args::StructuredArgs,
            category: Development,
            tags: [dev_tag::GENERAL],
            runner: runner_structured_try,
            summary: dynamic,
            description: "结构化数据处理，按 `action` 分派（**`action` 必填**）：`validate` 解析并校验 **JSON / YAML / TOML / CSV / TSV**（需 `path`；可选 `summarize` 顶层摘要、`format` 显式格式、`has_header` 表头）；`query` 在解析后的模型上按 **JSON Pointer**（`/a/b`）或**点号路径**（`a.b.0`）取值（需 `path` + `query`，比整文件 `read_file` 省上下文）；`diff` 把两份文件解析为同一模型后做**键路径级**差异（需 `path_a` + `path_b`；`max_diff_lines` 默认 200。非行级 diff，与 `text_diff` 互补）；`patch` 对 **JSON / YAML / TOML** 定点改写（需 `path` + `query` + `mutation`=`set`（默认，须同时给 `value`）/`remove`；**`dry_run` 默认 `true` 仅预览**，显式 `dry_run=false` 才写盘；`base_revision` 提供 CAS）。**只有本 `action` 认识的键会被采用**，其余字段忽略；CSV/TSV 支持校验/查询/diff，**不支持**结构化写回。单文件上限 4MiB。",
        },
]
