[
ToolSpec {
            name: "ci_pipeline_local",
            description: "本地一键执行 CI 关键检查（cargo fmt/clippy/test、frontend lint、可选 ruff/pytest/mypy）。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::CiPipelineLocalArgs>,
            runner: ToolRunner::Legacy(runner_ci_pipeline_local),
            summary: ToolSummaryKind::Static("local CI pipeline"),
        },
        ToolSpec {
            name: "release_ready_check",
            description: "发布前一键检查：CI + audit + deny + 工作区干净检查。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::ReleaseReadyCheckArgs>,
            runner: ToolRunner::Legacy(runner_release_ready_check),
            summary: ToolSummaryKind::Static("pre-release checks"),
        },
        ToolSpec {
            name: "workflow_execute",
            description: "执行 DAG 工作流：并行/串行调度 + 人工审批节点 + SLA 超时 + 失败补偿。\n\n【按文件执行】顶层 **`workflow_file`**：工作区相对路径（如 **`examples/workflows/ci.yaml`** 或工作区内路径；含 `` ```crabmate-workflow `` 的 `.md`）；服务端编译 `steps`/`when`/`for_each` 后执行。可与 **`workflow`** 叠加（覆盖 `fail_fast` 等，不覆盖 `nodes`/`steps`）。\n\n【内置模板】**`workflow.workflow_template`**：**`rust_ci_light`**、**`code_review`**、**`refactor_precheck`**（须 **`refactor_symbol`**）。可与手写 **`nodes`** 二选一；见 **`docs/工具说明.md`**、**`docs/工作流Markdown作者层设计.md`**。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::WorkflowExecuteArgs>,
            runner: ToolRunner::Legacy(runner_workflow_execute),
            summary: ToolSummaryKind::Static("DAG workflow"),
        },
        ToolSpec {
            name: "rust_backtrace_analyze",
            description: "分析 Rust panic/backtrace 文本，提取首个可疑业务帧和模块命中统计。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::BacktraceAnalyzeArgs>,
            runner: ToolRunner::Legacy(runner_backtrace_analyze),
            summary: ToolSummaryKind::Static("Rust backtrace analysis"),
        },
]
