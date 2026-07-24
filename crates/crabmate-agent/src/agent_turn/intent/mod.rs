//! 分阶段门控与意图上下文装配（无 IO）。

pub mod context;
pub mod readonly_overview_bypass;
pub mod simple_execute_fast_path;
pub mod user;

pub use context::build_intent_routing_context;
pub use user::{
    collect_recent_user_messages, extract_effective_user_task,
    recently_waiting_execute_confirmation,
};
