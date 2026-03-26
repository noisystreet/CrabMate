[
ToolSpec {
            name: "format_file",
            description: "对工作区内的文件进行代码格式化。根据文件扩展名自动选择合适的本地格式化器，例如 Rust 使用 rustfmt，C/C++ 使用 clang-format，前端 TypeScript/JavaScript 使用项目内的 Prettier，Python 使用 ruff format。适合在修改代码后统一整理缩进和风格。注意：需要本地已安装相应格式化工具。",
            category: ToolCategory::Development,
            parameters: tool_params::params_format_file,
            runner: runner_format_file,
            summary: ToolSummaryKind::None,
        },
        ToolSpec {
            name: "format_check_file",
            description: "对单个文件做格式检查（不修改磁盘）：Rust 使用 rustfmt --check，C/C++ 使用 clang-format --dry-run --Werror，前端类文件使用 prettier --check，Python 使用 ruff format --check。适合在提交前确认风格一致。",
            category: ToolCategory::Development,
            parameters: tool_params::params_format_check_file,
            runner: runner_format_check_file,
            summary: ToolSummaryKind::Dynamic(ts::summary_format_check_file),
        },
        ToolSpec {
            name: "run_lints",
            description: "运行项目的静态检查工具并聚合结果。目前包括：后端的 cargo clippy 和（若存在 frontend 目录与 package.json）前端的 npm run lint。可用于在改动后检查潜在问题。",
            category: ToolCategory::Development,
            parameters: tool_params::params_run_lints,
            runner: runner_run_lints,
            summary: ToolSummaryKind::None,
        },
        ToolSpec {
            name: "quality_workspace",
            description: "按开关组合运行质量检查：默认 cargo fmt --check + cargo clippy（轻量）；可选 cargo test、frontend npm lint、frontend prettier --check。适合「改完一轮后」快速拉齐格式与静态分析。",
            category: ToolCategory::Development,
            parameters: tool_params::params_quality_workspace,
            runner: runner_quality_workspace,
            summary: ToolSummaryKind::Static("工作区质量检查"),
        },
]
