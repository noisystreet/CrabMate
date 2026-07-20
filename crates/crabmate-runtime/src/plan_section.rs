//! 分阶段规划在 terminal/cli 显示中使用的段落标记与 user 文案片段。
//!
//! `STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX` 从 `crabmate_display_rules` 重导出。

/// 分阶段规划的**唯一**一级 section 标题（`###`）；用于 `message_display` 的语义增强与代理侧 `staged_sse::plan_section` Tag。
pub const STAGED_PLAN_SECTION_HEADER: &str = "### 分阶段规划 · 步骤概要";

/// 分阶段单步注入 user 的固定 boilerplate 结尾段（精简版，见 `staged_plan_prepare` / `plan_pipeline_schedule`）。
pub const STAGED_STEP_USER_BOILERPLATE: &str = "聚焦当前步，执行后续步骤。";

/// 分阶段自然语言回退循环的 user 消歧前缀（影响终端显示，不从消息中抑制）。
pub use crabmate_display_rules::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX;
