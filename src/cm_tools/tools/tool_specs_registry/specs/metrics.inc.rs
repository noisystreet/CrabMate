[
        builtin_tool! {
            name: tool_names::CODE_STATS,
            args: args::CodeStatsArgs,
            category: Development,
            tags: [dev_tag::GENERAL],
            runner: runner_code_stats_try,
            summary: dynamic,
            description: "统计工作区代码行数（按语言分类）。启用 **`project_metrics`** 构建时使用 tokei；否则按扩展名内置 walk 粗估（不含注释/空行拆分）。可选 path 指定子目录，format=table/json。",
        },
        builtin_tool! {
            name: tool_names::DEPENDENCY_GRAPH,
            args: args::DependencyGraphArgs,
            category: Development,
            tags: [dev_tag::GENERAL, dev_tag::RUST, dev_tag::FRONTEND, dev_tag::GO, dev_tag::JVM],
            runner: runner_dependency_graph_try,
            summary: dynamic,
            description: "生成项目依赖关系图（只读）。自动检测 Cargo.toml / go.mod / package.json；输出 format=mermaid（默认）/dot/tree。Cargo 项目基于 cargo tree，Go 基于 go list -m all，npm 基于 npm ls。",
        },
        builtin_tool! {
            name: tool_names::COVERAGE_REPORT,
            args: args::CoverageReportArgs,
            category: Development,
            tags: [dev_tag::GENERAL, dev_tag::RUST, dev_tag::FRONTEND, dev_tag::PYTHON, dev_tag::GO, dev_tag::QUALITY],
            runner: runner_coverage_report_try,
            summary: dynamic,
            description: "解析测试覆盖率报告并输出摘要（只读）。支持 LCOV（.info）、Tarpaulin JSON、Cobertura XML。可指定 path 或自动检测常见位置（lcov.info、coverage/、tarpaulin-report.json 等）。输出文件级覆盖率与总览百分比。",
        },
        // ── 文件工具增强 ────────────────────────────────────
]
