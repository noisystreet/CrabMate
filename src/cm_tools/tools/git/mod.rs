//! Git 工具：只读查询（写操作已收敛至 `run_command`）。
//!
//! 安全策略：
//! - 路径参数仅允许相对路径，禁止 `..` 与绝对路径
//! - 仅在当前工作区仓库内执行

mod helpers;
mod read_ops;

pub use helpers::ensure_git_repo;
pub use read_ops::{
    blame, branch_list, clean_check, diff, file_history, log, remote_list, remote_status, show,
    status,
};
