[
    ToolSpec {
        name: "run_lints",
        description: "运行项目的静态检查工具并聚合结果。目前包括：后端的 cargo clippy 和（若存在 frontend 目录与 package.json）前端的 npm run lint。可用于在改动后检查潜在问题。",
        category: ToolCategory::Development,
        parameters: schema_of::<args::RunLintsArgs>,
        runner: ToolRunner::Legacy(runner_run_lints),
        summary: ToolSummaryKind::None,
    },
    ToolSpec {
        name: "quality_workspace",
        description: "按开关组合运行质量检查：默认 cargo fmt --check + cargo clippy（轻量）；可选 cargo test、frontend npm lint、frontend prettier --check。适合「改完一轮后」快速拉齐格式与静态分析。",
        category: ToolCategory::Development,
        parameters: schema_of::<args::QualityWorkspaceArgs>,
        runner: ToolRunner::Legacy(runner_quality_workspace),
        summary: ToolSummaryKind::Static("workspace quality check"),
    },
]
