//! `[agent]` 的 `autonomy_*` 平铺键 → 运行态 [`AutonomyTail`]（`#1455`）。
//!
//! 单独成模块而不是塞在 `tail.rs`：这组已有 9 个键，散落在
//! `FinalizeTailScalars` 与 `TailPlanToolThinkingScalars` 两个结构里会让字段列表
//! 与文件行数一起膨胀（触及 file ≤ 920 行门禁）。收成嵌套结构后两处各只剩一行。

use crate::cm_config::builder::ConfigBuilder;

/// `autonomy_*` 的解析结果（默认值与钳制规则已在 [`derive_autonomy_tail`] 施加）。
#[derive(Debug, Clone, Copy)]
pub(super) struct AutonomyTail {
    pub(super) enabled: bool,
    pub(super) max_tasks: usize,
    pub(super) task_ttl_secs: u64,
    pub(super) max_cumulative_llm_calls: u32,
    pub(super) max_cumulative_tokens: usize,
    pub(super) max_cumulative_secs: u64,
    pub(super) auto_resume: bool,
    pub(super) max_autonomous_turns: u32,
    pub(super) stop_on_approval_required: bool,
}

/// 从 builder 解析本组配置。**默认值集中在此**，不要在别处再写一遍。
pub(super) fn derive_autonomy_tail(b: &ConfigBuilder) -> AutonomyTail {
    AutonomyTail {
        enabled: b.autonomy.enabled.unwrap_or(false),
        max_tasks: b.autonomy.max_tasks.unwrap_or(200).min(100_000) as usize,
        task_ttl_secs: b.autonomy.task_ttl_secs.unwrap_or(604_800).min(31_536_000),
        max_cumulative_llm_calls: b
            .autonomy
            .max_cumulative_llm_calls
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32,
        max_cumulative_tokens: b.autonomy.max_cumulative_tokens.unwrap_or(0) as usize,
        max_cumulative_secs: b.autonomy.max_cumulative_secs.unwrap_or(0),
        auto_resume: b.autonomy.auto_resume.unwrap_or(false),
        max_autonomous_turns: b
            .autonomy
            .max_autonomous_turns
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32,
        stop_on_approval_required: b.autonomy.stop_on_approval_required.unwrap_or(true),
    }
}
