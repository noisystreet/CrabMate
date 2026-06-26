//! Web / CLI 首轮或回合前的**上下文拼装**：living docs、项目画像、依赖摘要、**首条 system 动态组装**与会话 bootstrap。
//!
//! 与 [`crabmate_config`] 中的注入开关对应；路径解析依赖 [`crate::workspace`]。

pub mod conversation_turn_bootstrap;
pub mod living_docs;
pub mod project_dependency_brief;
pub mod project_profile;
pub mod prompt_compose;
