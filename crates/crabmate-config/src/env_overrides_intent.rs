//! `CM_INTENT_*` 环境变量覆盖（从 `env_overrides.rs` 拆分以降低圈复杂度）。

use crate::builder::ConfigBuilder;
use crate::source::parse_bool_like;

pub(super) fn env_override_intent_thresholds(b: &mut ConfigBuilder) {
    intent_override_turn_start_and_l2(b);
    intent_override_execute_thresholds(b);
    intent_override_non_hier_execute_thresholds(b);
    intent_override_mode_bias(b);
}

fn intent_override_turn_start_and_l2(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_INTENT_L2_MIN_CONFIDENCE")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_l2_min_confidence = Some(f);
    }
    if let Ok(v) = std::env::var("CM_INTENT_L2_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.intent_routing.intent_l2_max_tokens = Some(n);
    }
    if let Ok(v) = std::env::var("CM_INTENT_L0_ROUTING_BOOST_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.intent_routing.intent_l0_routing_boost_enabled = Some(val);
    }
}

fn intent_override_execute_thresholds(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_INTENT_EXECUTE_LOW_THRESHOLD")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_execute_low_threshold = Some(f);
    }
    if let Ok(v) = std::env::var("CM_INTENT_EXECUTE_HIGH_THRESHOLD")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_execute_high_threshold = Some(f);
    }
}

fn intent_override_non_hier_execute_thresholds(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_INTENT_NON_HIER_EXECUTE_LOW_THRESHOLD")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_non_hier_execute_low_threshold = Some(f);
    }
    if let Ok(v) = std::env::var("CM_INTENT_NON_HIER_EXECUTE_HIGH_THRESHOLD")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_non_hier_execute_high_threshold = Some(f);
    }
}

fn intent_override_mode_bias(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_INTENT_MODE_BIAS_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.intent_routing.intent_mode_bias_enabled = Some(val);
    }
}
