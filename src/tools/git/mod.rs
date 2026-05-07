//! Git 工具：只读查询 + 受控写入（stage/commit）
//!
//! 安全策略：
//! - 路径参数仅允许相对路径，禁止 `..` 与绝对路径
//! - commit 必须显式 confirm=true 才执行
//! - 仅在当前工作区仓库内执行

mod helpers;
mod read_ops;
mod write_ops;

pub(crate) use helpers::ensure_git_repo;
pub use read_ops::{
    apply, blame, branch_list, clean_check, clone_repo, commit, diff, diff_base, diff_names,
    diff_stat, fetch, file_history, log, remote_list, remote_set_url, remote_status, show,
    stage_files, status,
};
pub use write_ops::{
    branch_create, branch_delete, checkout, cherry_pick, merge, push, rebase, reset, revert, stash,
    tag,
};
