//! GitHub CLI（`gh_*`）工具的 `runner_*` 薄封装。

use super::*;

macro_rules! gh_runner {
    ($name:ident, $fn:path) => {
        pub fn $name(args: &str, ctx: &ToolContext<'_>) -> String {
            $fn(
                args,
                ctx.command_max_output_len,
                ctx.allowed_commands,
                ctx.working_dir,
            )
        }
    };
}

gh_runner!(runner_gh_pr_list, github_cli::gh_pr_list);
gh_runner!(runner_gh_pr_view, github_cli::gh_pr_view);
gh_runner!(runner_gh_pr_checks, github_cli::gh_pr_checks);
gh_runner!(runner_gh_pr_create, github_cli::gh_pr_create);
gh_runner!(runner_gh_pr_merge, github_cli::gh_pr_merge);
gh_runner!(runner_gh_pr_review, github_cli::gh_pr_review);
gh_runner!(runner_gh_pr_comment, github_cli::gh_pr_comment);
gh_runner!(runner_gh_issue_list, github_cli::gh_issue_list);
gh_runner!(runner_gh_issue_view, github_cli::gh_issue_view);
gh_runner!(runner_gh_issue_create, github_cli::gh_issue_create);
gh_runner!(runner_gh_run_list, github_cli::gh_run_list);
gh_runner!(runner_gh_pr_diff, github_cli::gh_pr_diff);
gh_runner!(runner_gh_run_view, github_cli::gh_run_view);
gh_runner!(runner_gh_run_rerun, github_cli::gh_run_rerun);
gh_runner!(
    runner_gh_run_failure_summary,
    github_cli::gh_run_failure_summary
);
gh_runner!(runner_gh_release_list, github_cli::gh_release_list);
gh_runner!(runner_gh_release_view, github_cli::gh_release_view);
gh_runner!(runner_gh_release_create, github_cli::gh_release_create);
gh_runner!(runner_gh_search, github_cli::gh_search);
gh_runner!(runner_gh_api, github_cli::gh_api);

pub fn runner_gh_pr_body_draft(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_body_draft(args, ctx.working_dir, ctx.command_max_output_len)
}
