//! Web / CLI 首轮或回合前的**上下文拼装**：living docs、依赖摘要、**首条 system 动态组装**与会话 bootstrap。
//! 项目画像（[`crate::cm_tools::project_profile`]）不在此处注入，仅供 `GET /workspace/profile` 与 `repo_overview_sweep` 按需使用。
//!
//! 与 [`crate::cm_config`] 中的注入开关对应；路径解析依赖 [`crate::cm_internal::workspace`]。

pub mod agents_md;
pub mod conversation_turn_bootstrap;
pub mod first_turn_inject;
pub mod living_docs;
pub mod project_dependency_brief;
pub mod prompt_compose;

pub use crate::cm_tools::project_profile;
pub use first_turn_inject::{build_first_turn_user_context_markdown, merge_first_turn_injections};
