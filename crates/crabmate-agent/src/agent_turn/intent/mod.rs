//! 意图用户消息侧辅助（无 IO）。

pub mod user;

pub use user::{extract_effective_user_task, recently_waiting_execute_confirmation};
