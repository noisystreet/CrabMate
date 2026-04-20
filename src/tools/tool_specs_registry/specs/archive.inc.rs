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
        description: "解压归档文件（tar、zip、tar.gz、tar.bz2、tar.xz、7z、rar）。自动检测格式，支持指定输出目录。",
        category: ToolCategory::Development,
        parameters: tool_params::params_archive_unpack,
        runner: runner_archive_unpack,
        summary: ToolSummaryKind::Dynamic(ts::summary_archive_unpack),
    },
    ToolSpec {
        name: "archive_list",
        description: "列出归档内容（不解压）。支持 tar、zip、tar.gz、tar.bz2、tar.xz、7z、rar 格式。显示文件列表、大小、修改时间。",
        category: ToolCategory::Development,
        parameters: tool_params::params_archive_list,
        runner: runner_archive_list,
        summary: ToolSummaryKind::Dynamic(ts::summary_archive_list),
    },
]
