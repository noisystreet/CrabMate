//! `CM_REFLECTION_*` / `CM_FINAL_PLAN_*` / `CM_PLANNER_EXECUTOR_MODE` 等 per-plan 策略环境覆盖（从 `env_overrides.rs` 拆分以降低圈复杂度）。

use crate::config::builder::ConfigBuilder;
use crate::config::source::parse_bool_like;

pub(super) fn env_override_reflection_and_final_plan(b: &mut ConfigBuilder) {
    env_override_reflection_rounds_and_rewrite(b);
    env_override_final_plan_flags(b);
    env_override_planner_executor_mode_str(b);
}

fn env_override_reflection_rounds_and_rewrite(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_REFLECTION_DEFAULT_MAX_ROUNDS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.per_plan_policy.reflection_default_max_rounds = Some(n);
    }
    if let Ok(s) = std::env::var("CM_FINAL_PLAN_REQUIREMENT") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.per_plan_policy.final_plan_requirement_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("CM_PLAN_REWRITE_MAX_ATTEMPTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.per_plan_policy.plan_rewrite_max_attempts = Some(n);
    }
}

fn env_override_final_plan_flags(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_FINAL_PLAN_REQUIRE_STRICT_WORKFLOW_NODE_COVERAGE")
        && let Some(val) = parse_bool_like(&v)
    {
        b.per_plan_policy
            .final_plan_require_strict_workflow_node_coverage = Some(val);
    }
    if let Ok(v) = std::env::var("CM_FINAL_PLAN_SEMANTIC_CHECK_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.per_plan_policy.final_plan_semantic_check_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_FINAL_PLAN_SEMANTIC_CHECK_MAX_NON_READONLY_TOOLS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.per_plan_policy
            .final_plan_semantic_check_max_non_readonly_tools = Some(n);
    }
    if let Ok(v) = std::env::var("CM_FINAL_PLAN_SEMANTIC_CHECK_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.per_plan_policy.final_plan_semantic_check_max_tokens = Some(n);
    }
}

fn env_override_planner_executor_mode_str(b: &mut ConfigBuilder) {
    if let Ok(s) = std::env::var("CM_PLANNER_EXECUTOR_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.per_plan_policy.planner_executor_mode_str = Some(s);
        }
    }
}
