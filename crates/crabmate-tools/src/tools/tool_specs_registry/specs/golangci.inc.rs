[
ToolSpec {
            name: "golangci_lint",
            description: "运行 golangci-lint run（需已安装 golangci-lint 且存在 go.mod）。可选 fix、fast。\n\n【golangci-lint 常用模式】检查所有包：`golangci-lint run ./...`；修复自动可修复的问题：`golangci-lint run --fix ./...`；仅检查新问题（快速模式）：`golangci-lint run --fast ./...`。",
            category: ToolCategory::Development,
            parameters: tool_params::params_golangci_lint,
            runner: runner_golangci_lint,
            summary: ToolSummaryKind::Static("golangci-lint"),
        },
]
