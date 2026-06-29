//! 分层任务收尾：任务级验收与「关键证据」Markdown（从 `hierarchy.rs` 拆出以降低圈复杂度）。
//!
//! 拆为 `common` / `verify` / `render` 子模块，避免单文件内 `r#""#` 导致 lizard 将后续函数误并为一条度量。

mod common;
mod render;
mod verify;

pub(crate) use common::is_program_build_run_request;
pub(crate) use render::render_task_level_evidence;
pub(crate) use verify::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
    generic_task_intent_implies_build_or_test, verify_task_level_execution_evidence,
};
