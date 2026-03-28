[
ToolSpec {
            name: "golangci_lint",
            description: "运行 golangci-lint run（需已安装 golangci-lint 且存在 go.mod）。可选 fix、fast。",
            category: ToolCategory::Development,
            parameters: tool_params::params_golangci_lint,
            runner: runner_golangci_lint,
            summary: ToolSummaryKind::Static("golangci-lint"),
        },
]
