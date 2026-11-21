//! `CM_AUTONOMY_*`：L2 跨回合任务状态（`#1386` S2 / `#1455` A 步）的环境覆盖。
//!
//! 默认关：`enabled = false` 时预算耗尽后**不落盘**，行为与接线前逐字一致。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_parse};

pub(super) fn apply(b: &mut ConfigBuilder) {
    apply_bool(&mut b.autonomy.enabled, "CM_AUTONOMY_ENABLED");
    apply_parse(&mut b.autonomy.max_tasks, "CM_AUTONOMY_MAX_TASKS");
    apply_parse(
        &mut b.autonomy.task_ttl_secs,
        "CM_AUTONOMY_TASK_TTL_SECS",
    );
    // B 步（#1455）：跨回合累计三项上限，**软闸**，默认 0 = 不限制。
    apply_parse(
        &mut b.autonomy.max_cumulative_llm_calls,
        "CM_AUTONOMY_MAX_CUMULATIVE_LLM_CALLS",
    );
    apply_parse(
        &mut b.autonomy.max_cumulative_tokens,
        "CM_AUTONOMY_MAX_CUMULATIVE_TOKENS",
    );
    apply_parse(
        &mut b.autonomy.max_cumulative_secs,
        "CM_AUTONOMY_MAX_CUMULATIVE_SECS",
    );
    // C 步（#1455）：自动续跑开关与硬闸。
    apply_bool(
        &mut b.autonomy.auto_resume,
        "CM_AUTONOMY_AUTO_RESUME",
    );
    apply_parse(
        &mut b.autonomy.max_autonomous_turns,
        "CM_AUTONOMY_MAX_AUTONOMOUS_TURNS",
    );
    apply_bool(
        &mut b.autonomy.stop_on_approval_required,
        "CM_AUTONOMY_STOP_ON_APPROVAL_REQUIRED",
    );
}
