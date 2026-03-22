//! 分阶段规划在 TUI 主聊天区、CLI 转录与 `staged_plan_summary_text` 中的**节标题**统一入口（与「【规划】共 N 步」正文前缀一致）。

/// 规划块顶栏（TUI `push_staged_plan_chat_block` 首行、CLI `clear_before` 时打印的节标题）。
pub const STAGED_PLAN_SECTION_HEADER: &str = "【规划】";
