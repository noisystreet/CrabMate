[
ToolSpec {
            name: tool_names::DELETE_FILES,
            description: "删除工作区内一个或多个文件（受控写入，需 confirm=true）。`paths` 为相对路径列表（最多 32 个，同路径去重）；任一路径非法则整批拒绝、不产生部分删除；不能删除目录（请用 delete_dir）。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::DeleteFilesArgs>,
            runner: ToolRunner::Typed(runner_delete_files_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_delete_files),
        },
        ToolSpec {
            name: tool_names::DELETE_DIR,
            description: "删除工作区内的目录（受控写入，需 confirm=true）。默认仅删除空目录；recursive=true 时递归删除含子目录和文件。不能删除工作区根目录。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::DeleteDirArgs>,
            runner: ToolRunner::Typed(runner_delete_dir_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_delete_dir),
        },
        ToolSpec {
            name: tool_names::APPEND_FILE,
            description: "在工作区内文件末尾追加内容。默认文件须已存在；create_if_missing=true 时不存在则创建。不覆盖已有内容。支持 dry_run 预览；**dry_run 默认 true 只预览**，显式 dry_run=false 才写盘。默认 ensure_leading_newline=true，会在原文件非空且末尾无换行时先补一个换行，避免新内容接在旧末行后。`content` 可用 body/text/code 等别名。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::AppendFileArgs>,
            runner: ToolRunner::Typed(runner_append_file_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_append_file),
        },
        ToolSpec {
            name: tool_names::CREATE_DIR,
            description: "在工作区内创建目录。默认 parents=true（递归创建父目录，类似 mkdir -p）。目录已存在时不报错。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::CreateDirArgs>,
            runner: ToolRunner::Typed(runner_create_dir_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_create_dir),
        },
        ToolSpec {
            name: tool_names::SEARCH_REPLACE,
            description: "在工作区内**单个文件**中搜索并替换文本，**局部内容改动的首选**（按文本锚定、无需行号，不受行号漂移影响；比按行号编辑的 `modify_file` 更不易改错行）。支持字面量或正则表达式匹配（正则保留 `$1` 展开）。**默认要求 search 在文件中唯一匹配**：命中多处即拒写并列出各命中行号（未写盘），请补足上下文让锚点唯一，或显式声明范围——`max_replacements=N` 只改前 N 处，`replace_all=true`（等价 `max_replacements=0`）全部替换。**dry_run 默认 true 仅预览**（返回 unified diff）；显式 dry_run=false 即写盘，不再要求 confirm（该字段仅为兼容保留）。可选 `base_revision` 做写前 CAS（本轮 read_file 有绑定也会自动校验）。文件上限 4MiB。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::SearchReplaceArgs>,
            runner: ToolRunner::Typed(runner_search_replace_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_search_replace),
        },
        // ── 新增纯内存 / 开发辅助工具 ────────────────────────
]
