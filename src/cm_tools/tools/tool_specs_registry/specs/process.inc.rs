[
ToolSpec {
            name: "port_check",
            description: "检查指定端口是否被占用（只读，使用 ss/lsof）。返回占用该端口的进程信息。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::PortCheckArgs>,
            runner: ToolRunner::Legacy(runner_port_check),
            summary: ToolSummaryKind::Dynamic(ts::summary_port_check),
        },
        ToolSpec {
            name: "process_list",
            description: "列出系统进程（只读，使用 ps）。可按关键词过滤、限制返回条数。默认仅当前用户进程。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::ProcessListArgs>,
            runner: ToolRunner::Legacy(runner_process_list),
            summary: ToolSummaryKind::Dynamic(ts::summary_process_list),
        },
        ToolSpec {
            name: "background_job_status",
            description: "查询后台工具任务的状态与输出（只读，不走白名单审批）。用于 run_command 以 async: true 启动的长任务：传入 tool_job_id；任务未结束时返回当前状态（稍后再查即可），结束时返回退出码与 stdout/stderr（超长会被截断）。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::BackgroundJobStatusArgs>,
            runner: ToolRunner::Legacy(runner_background_job_status),
            summary: ToolSummaryKind::Dynamic(ts::summary_background_job_status),
        },
        ToolSpec {
            name: "background_job_list",
            description: "列出当前工作区的后台工具任务（只读，最新创建在前），含任务 id、状态与命令摘要。可用返回的 id 调用 background_job_status 查看详情。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::BackgroundJobListArgs>,
            runner: ToolRunner::Legacy(runner_background_job_list),
            summary: ToolSummaryKind::Dynamic(ts::summary_background_job_list),
        },
        // ── 代码度量与分析 ──────────────────────────────────
]
