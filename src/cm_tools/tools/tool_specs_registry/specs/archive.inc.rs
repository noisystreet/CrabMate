[
    ToolSpec {
        name: "archive_pack",
        description: "创建压缩归档（tar、zip、tar.gz、tar.bz2、tar.xz）。支持打包文件或目录。自动处理路径，支持排除模式。",
        category: ToolCategory::Development,
        parameters: tool_params::params_archive_pack,
        runner: runner_archive_pack,
        summary: ToolSummaryKind::Dynamic(ts::summary_archive_pack),
    },
    ToolSpec {
        name: "archive_unpack",
        description: "解压归档文件（tar、zip、tar.gz、tar.bz2、tar.xz、7z、rar）。自动检测格式。源码包默认 `output_dir=\".\"` 解压到工作区根；若归档内仅一层根目录且需扁平化可设 `strip_components=1`。勿自创嵌套目录名（如 `hpcg-3.1`）以免路径加深。",
        category: ToolCategory::Development,
        parameters: tool_params::params_archive_unpack,
        runner: runner_archive_unpack,
        summary: ToolSummaryKind::Dynamic(ts::summary_archive_unpack),
    },
    ToolSpec {
        name: "archive_list",
        description: "列出归档内容（不解压）。超大归档默认最多 250 项并附顶层摘要；可用 `max_entries` 调整。解压前建议先列顶层目录名。",
        category: ToolCategory::Development,
        parameters: tool_params::params_archive_list,
        runner: runner_archive_list,
        summary: ToolSummaryKind::Dynamic(ts::summary_archive_list),
    },
]
