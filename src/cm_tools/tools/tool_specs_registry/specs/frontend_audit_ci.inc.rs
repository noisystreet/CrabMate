[
ToolSpec {
            name: tool_names::WORKFLOW_EXECUTE,
            description: "执行 DAG 工作流：并行/串行调度 + 人工审批节点 + SLA 超时 + 失败补偿。\n\n【按文件执行】顶层 **`workflow_file`**：工作区相对路径（如 **`examples/workflows/ci.yaml`** 或工作区内路径；含 `` ```crabmate-workflow `` 的 `.md`）；服务端编译 `steps`/`when`/`for_each` 后执行。可与 **`workflow`** 叠加（覆盖 `fail_fast` 等，不覆盖 `nodes`/`steps`）。\n\n【内置模板】**`workflow.workflow_template`**：**`rust_ci_light`**、**`code_review`**、**`refactor_precheck`**（须 **`refactor_symbol`**）。可与手写 **`nodes`** 二选一；见 **`docs/工具说明.md`**、**`docs/工作流Markdown作者层设计.md`**。",
            category: ToolCategory::Development,
            tags: &[dev_tag::GENERAL, dev_tag::RUST, dev_tag::QUALITY],
            parameters: schema_of::<args::WorkflowExecuteArgs>,
            runner: ToolRunner::Typed(runner_workflow_execute_try),
            summary: ToolSummaryKind::Static("DAG workflow"),
        },
]
