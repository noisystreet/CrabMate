[
ToolSpec {
            name: "code_stats",
            description: "统计工作区代码行数（按语言分类）。优先使用 tokei，回退 cloc，均未安装时使用内置统计（按扩展名识别语言、估算注释/空行/代码行）。可选 path 指定子目录，format=table/json。",
            category: ToolCategory::Development,
            parameters: tool_params::params_code_stats,
            runner: runner_code_stats,
            summary: ToolSummaryKind::Dynamic(ts::summary_code_stats),
        },
        ToolSpec {
            name: "dependency_graph",
            description: "生成项目依赖关系图（只读）。自动检测 Cargo.toml / go.mod / package.json；输出 format=mermaid（默认）/dot/tree。Cargo 项目基于 cargo tree，Go 基于 go list -m all，npm 基于 npm ls。",
            category: ToolCategory::Development,
            parameters: tool_params::params_dependency_graph,
            runner: runner_dependency_graph,
            summary: ToolSummaryKind::Dynamic(ts::summary_dependency_graph),
        },
        ToolSpec {
            name: "coverage_report",
            description: "解析测试覆盖率报告并输出摘要（只读）。支持 LCOV（.info）、Tarpaulin JSON、Cobertura XML。可指定 path 或自动检测常见位置（lcov.info、coverage/、tarpaulin-report.json 等）。输出文件级覆盖率与总览百分比。",
            category: ToolCategory::Development,
            parameters: tool_params::params_coverage_report,
            runner: runner_coverage_report,
            summary: ToolSummaryKind::Dynamic(ts::summary_coverage_report),
        },
        // ── 文件工具增强 ────────────────────────────────────
]
