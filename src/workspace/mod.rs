//! 工作区路径策略、根目录内安全打开（`openat2` 等）与会话级变更集注入。
//!
//! - [`path`]：路径校验、解析与 Web/工具共用边界。
//! - [`fs`]：Unix/Linux 下在已打开根 fd 上打开文件/目录。
//! - [`changelist`]：按会话作用域累积写入并注入模型上下文。

pub mod changelist;
pub mod fs;
pub mod path;
pub mod tasks_side;
