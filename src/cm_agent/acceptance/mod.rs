//! 解析期给 `test_runner` 步注入缺省 `expect_exit_code`。
//!
//! **验证内核已移除**（#1429）：`AcceptanceSpec` / `AcceptanceEvidence` / `VerifyOutcome` /
//! `verify_against_spec` / `verify_plan_step_acceptance_for_tool_message` /
//! `parse_exit_code_from_combined_output` 的消费者（分阶段 `step_verifier`、分层 `GoalVerifier`）
//! 已随 `chore(staged-removal)` 删除，这层只剩 golden 与单测引用，故整体移除。
//!
//! 现存只有 [`apply_executor_kind_acceptance_defaults`]：解析期由 `plan_artifact/parse.rs` 调用。
//! 注意 `PlanStepAcceptance::is_effective` 名字相近，但它定义在 `plan_artifact`，
//! 仍被 `completion_suppression` 用于冗余抑制判定。

mod executor_defaults;

pub use executor_defaults::{DEFAULT_COMMAND_EXIT_CODE, apply_executor_kind_acceptance_defaults};
