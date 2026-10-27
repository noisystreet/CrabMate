//! Git 工具：只读查询（写操作已收敛至 `run_command`）。
//!
//! 对外只暴露两样东西：`ensure_git_repo`（`gh_pr` 等需要预检仓库）
//! 与 `dispatch`（`git` 工具的唯一入口）。十个子命令实现留在 `read_ops` 内部，
//! 不导出——它们只该经 `action` 选择器抵达，不该被当作独立 API 调用。
//!
//! 安全策略：
//! - 路径参数仅允许相对路径，禁止 `..` 与绝对路径
//! - 仅在当前工作区仓库内执行

mod helpers;
mod read_ops;

pub use helpers::ensure_git_repo;
pub use read_ops::dispatch;
