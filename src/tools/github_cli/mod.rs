//! 内置 GitHub CLI（`gh`）封装：结构化参数；**退出码 0** 且 **stdout 整段为合法 JSON** 时附加格式化块（与是否传入 `--json` 字段无关）。
//!
//! 须 **`allowed_commands` 含 `gh`**（嵌入默认已含）。写远端工具：**`gh_api`**（变更类 HTTP）、**`gh_pr_create`** / **`gh_pr_merge`** / **`gh_pr_review`** / **`gh_pr_comment`**、**`gh_issue_create`**、**`gh_run_rerun`**、**`gh_release_create`** 已列入写副作用工具集。

mod api;
mod common;
mod issue_create;
mod pr_body;
mod pr_issue;
mod pr_mutate;
mod pr_workflow;
mod release_create;
mod run_ci;
mod run_release_search;

pub(crate) use common::{
    attach_json_if_exit_zero, gh_allowed, validate_api_path, validate_extra_args,
};

pub use api::gh_api;
pub use issue_create::gh_issue_create;
pub use pr_body::gh_pr_body_draft;
pub use pr_issue::{gh_issue_list, gh_issue_view, gh_pr_list, gh_pr_view};
pub use pr_mutate::{gh_pr_comment, gh_pr_merge, gh_pr_review};
pub use pr_workflow::{gh_pr_checks, gh_pr_create, gh_pr_diff, gh_run_list};
pub use release_create::gh_release_create;
pub use run_ci::{gh_run_failure_summary, gh_run_rerun};
pub use run_release_search::{gh_release_list, gh_release_view, gh_run_view, gh_search};
