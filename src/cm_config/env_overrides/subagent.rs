//! `CM_SUBAGENT_*`：子 Agent 委托（#839 P1）的环境覆盖。
//!
//! 默认全关：`enabled = false` 时 `delegate_task` **不注入给模型**，因此不存在
//! 「工具可见但永远失败」的中间态。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_parse};

pub(super) fn apply(b: &mut ConfigBuilder) {
    apply_bool(&mut b.subagent.enabled, "CM_SUBAGENT_ENABLED");
    apply_parse(&mut b.subagent.max_depth, "CM_SUBAGENT_MAX_DEPTH");
    apply_parse(&mut b.subagent.max_llm_calls, "CM_SUBAGENT_MAX_LLM_CALLS");
    apply_parse(&mut b.subagent.max_wall_secs, "CM_SUBAGENT_MAX_WALL_SECS");
    apply_parse(
        &mut b.subagent.max_report_chars,
        "CM_SUBAGENT_MAX_REPORT_CHARS",
    );
}
