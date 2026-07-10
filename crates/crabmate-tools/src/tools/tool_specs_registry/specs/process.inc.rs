[
ToolSpec {
            name: "port_check",
            description: "检查指定端口是否被占用（只读，使用 ss/lsof）。返回占用该端口的进程信息。",
            category: ToolCategory::Development,
            parameters: tool_params::params_port_check,
            runner: runner_port_check,
            summary: ToolSummaryKind::Dynamic(ts::summary_port_check),
        },
        ToolSpec {
            name: "process_list",
            description: "列出系统进程（只读，使用 ps）。可按关键词过滤、限制返回条数。默认仅当前用户进程。",
            category: ToolCategory::Development,
            parameters: tool_params::params_process_list,
            runner: runner_process_list,
            summary: ToolSummaryKind::Dynamic(ts::summary_process_list),
        },
        // ── 代码度量与分析 ──────────────────────────────────
]
