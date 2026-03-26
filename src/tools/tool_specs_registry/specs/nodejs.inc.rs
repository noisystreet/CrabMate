[
ToolSpec {
            name: "npm_install",
            description: "在工作区（或指定子目录）运行 npm install（或 npm ci）。须存在 package.json。可选 ci（干净安装）、production。",
            category: ToolCategory::Development,
            parameters: tool_params::params_npm_install,
            runner: runner_npm_install,
            summary: ToolSummaryKind::Static("运行 npm install"),
        },
        ToolSpec {
            name: "npm_run",
            description: "在工作区（或指定子目录）运行 npm run <script>。须存在 package.json。可传 args 到脚本（-- 之后）。",
            category: ToolCategory::Development,
            parameters: tool_params::params_npm_run,
            runner: runner_npm_run,
            summary: ToolSummaryKind::Dynamic(ts::summary_npm_run),
        },
        ToolSpec {
            name: "npx_run",
            description: "在工作区（或指定子目录）运行 npx <package>（自动安装执行）。须存在 package.json。如 npx prettier --check .、npx eslint 等。",
            category: ToolCategory::Development,
            parameters: tool_params::params_npx_run,
            runner: runner_npx_run,
            summary: ToolSummaryKind::Dynamic(ts::summary_npx_run),
        },
        ToolSpec {
            name: "tsc_check",
            description: "在工作区（或指定子目录）运行 TypeScript 类型检查（npx tsc --noEmit）。须存在 package.json 或 tsconfig.json。可选 project、strict。",
            category: ToolCategory::Development,
            parameters: tool_params::params_tsc_check,
            runner: runner_tsc_check,
            summary: ToolSummaryKind::Static("运行 tsc --noEmit"),
        },
        // ── Go 补充：golangci-lint ────────────────────────────
]
