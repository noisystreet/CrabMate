[
ToolSpec {
            name: "diagnostic_summary",
            description: "只读排障摘要：**Rust 工具链**（rustc/cargo -V、rustc -vV 的 host/release、rustup default、bc 是否可用）、**工作区**（根路径、`target/` 是否存在、`Cargo.toml` / `frontend/package.json` / `frontend/dist` 是否存在）、**环境变量仅状态**（`API_KEY`、常见 `AGENT_*`、`RUST_LOG` 等：未设置/空/非空；**永不输出变量值**；密钥类亦不输出长度）。可选 `extra_env_vars`（大写安全名）。与 AGENTS.md 排障场景一致。",
            category: ToolCategory::Development,
            parameters: tool_params::params_diagnostic_summary,
            runner: runner_diagnostic_summary,
            summary: ToolSummaryKind::Static("环境/工具链诊断摘要（脱敏）"),
        },
        ToolSpec {
            name: "changelog_draft",
            description: "根据 **git log** 生成 **Markdown 变更说明草稿**（**不写仓库**）。支持按提交日聚合 subject、`flat` 平铺、或 `tag_ranges` 按 semver 降序相邻 tag 分段（`--no-merges`）。可选 since/until 与 max_commits。",
            category: ToolCategory::Development,
            parameters: tool_params::params_changelog_draft,
            runner: runner_changelog_draft,
            summary: ToolSummaryKind::Static("生成变更日志 Markdown 草稿"),
        },
        ToolSpec {
            name: "license_notice",
            description: "运行 **cargo metadata** 解析依赖图，生成 **crate → license** 的 Markdown 表（**只读**；未在 Cargo.toml 声明的显示占位说明）。可选仅工作区成员、限制行数。非法律意见，发版前需人工核对。",
            category: ToolCategory::Development,
            parameters: tool_params::params_license_notice,
            runner: runner_license_notice,
            summary: ToolSummaryKind::Static("依赖许可证摘要表（cargo metadata）"),
        },
]
