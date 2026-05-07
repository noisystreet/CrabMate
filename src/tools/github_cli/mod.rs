//! 内置 GitHub CLI（`gh`）封装：结构化参数；**退出码 0** 且 **stdout 整段为合法 JSON** 时附加格式化块（与是否传入 `--json` 字段无关）。
//!
//! 须 **`allowed_commands` 含 `gh`**（嵌入默认已含）。**`gh_api`** 在变更类 HTTP 方法下可能修改远端资源；**`gh_pr_create`** 在 GitHub 上创建 PR，二者已列入写副作用工具集。

mod api;
mod common;
mod pr_issue;
mod pr_workflow;
mod run_release_search;

pub(crate) use common::{
    attach_json_if_exit_zero, gh_allowed, validate_api_path, validate_extra_args,
};

pub use api::gh_api;
pub use pr_issue::{gh_issue_list, gh_issue_view, gh_pr_list, gh_pr_view};
pub use pr_workflow::{gh_pr_checks, gh_pr_create, gh_pr_diff, gh_run_list};
pub use run_release_search::{gh_release_list, gh_release_view, gh_run_view, gh_search};
