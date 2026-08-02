//! `CM_INTENT_*` 环境变量覆盖（从 `env_overrides.rs` 拆分以降低圈复杂度）。

use crate::builder::ConfigBuilder;
use crate::source::parse_bool_like;

pub(super) fn env_override_intent(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_INTENT_AT_TURN_START_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.intent_routing.intent_at_turn_start_enabled = Some(val);
    }
}
