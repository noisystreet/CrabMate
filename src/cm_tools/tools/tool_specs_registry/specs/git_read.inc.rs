[
ToolSpec {
            name: tool_names::GIT,
            description: "只读 Git 查询。**`action` 必填**：`status`（可 `porcelain` / `include_untracked` / `branch`）、`clean_check`（是否干净）、`diff`（`mode`=working|staged|all、`path`、`context_lines`、`stat`、`name_only`、`base` → 此时对比 `base...HEAD`）、`log`（`max_count` 默认 20、`oneline`）、`show`（`rev` 默认 HEAD）、`blame`（`path` 必填，可选 `start_line`/`end_line`）、`file_history`（`path` 必填，`--follow`）、`branch_list`（`include_remote`）、`remote_status`（`git status -sb`）、`remote_list`（`git remote -v`）。写操作请用 `run_command`。",
            category: ToolCategory::Development,
            tags: &[dev_tag::GENERAL, dev_tag::VCS],
            parameters: schema_of::<args::GitArgs>,
            runner: ToolRunner::Typed(runner_git_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_git),
        },
]
