//! `CM_REFLECTION_*` / `CM_FINAL_PLAN_*` / `CM_PLANNER_EXECUTOR_MODE` 等 per-plan 策略环境覆盖。
//!
//! `CM_ORCHESTRATION_PROFILE` 仍忽略（运行时固定 ReAct）。`CM_PLANNER_EXECUTOR_MODE` 若设置则须为
//! `single_agent`，由 finalize 校验。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_nonempty_opt, apply_parse};

pub(super) fn env_override_reflection_and_final_plan(b: &mut ConfigBuilder) {
    env_override_reflection_rounds_and_rewrite(b);
    env_override_final_plan_flags(b);
    env_override_planner_executor_mode(b);
}

fn env_override_planner_executor_mode(b: &mut ConfigBuilder) {
    apply_nonempty_opt(
        &mut b.per_plan_policy.planner_executor_mode_str,
        "CM_PLANNER_EXECUTOR_MODE",
    );
}

fn env_override_reflection_rounds_and_rewrite(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.per_plan_policy.reflection_default_max_rounds,
        "CM_REFLECTION_DEFAULT_MAX_ROUNDS",
    );
    apply_nonempty_opt(
        &mut b.per_plan_policy.final_plan_requirement_str,
        "CM_FINAL_PLAN_REQUIREMENT",
    );
    apply_parse(
        &mut b.per_plan_policy.plan_rewrite_max_attempts,
        "CM_PLAN_REWRITE_MAX_ATTEMPTS",
    );
}

fn env_override_final_plan_flags(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b
            .per_plan_policy
            .final_plan_require_strict_workflow_node_coverage,
        "CM_FINAL_PLAN_REQUIRE_STRICT_WORKFLOW_NODE_COVERAGE",
    );
    apply_bool(
        &mut b.per_plan_policy.final_plan_semantic_check_enabled,
        "CM_FINAL_PLAN_SEMANTIC_CHECK_ENABLED",
    );
    apply_bool(
        &mut b
            .per_plan_policy
            .final_plan_semantic_check_accept_legacy_text,
        "CM_FINAL_PLAN_SEMANTIC_CHECK_ACCEPT_LEGACY_TEXT",
    );
    apply_parse(
        &mut b
            .per_plan_policy
            .final_plan_semantic_check_max_non_readonly_tools,
        "CM_FINAL_PLAN_SEMANTIC_CHECK_MAX_NON_READONLY_TOOLS",
    );
    apply_parse(
        &mut b.per_plan_policy.final_plan_semantic_check_max_tokens,
        "CM_FINAL_PLAN_SEMANTIC_CHECK_MAX_TOKENS",
    );
}
