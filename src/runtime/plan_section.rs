//! 分阶段规划在 CLI 转录与 `staged_plan_queue_summary_text` 中的**节标题**统一入口（与「`**规划** ·` 共 N 步」正文前缀一致）；TUI 规划行仅在右栏「队列」页展示（步骤行内 `[ ]`/`[✓]` 进度），主聊天区不再重复插入该标题块。

/// 规划摘要首行前缀（`staged_plan_queue_summary_text` 与协议示例一致）；CLI 在 `clear_before` 时对**首条非空展示行**着色，不再单独多打一行本常量。
pub const STAGED_PLAN_SECTION_HEADER: &str = "**规划** · ";

/// 分步执行注入的 `user` 消息中、紧跟在 `### 分步 i/n` 标题行后的那句模型约定说明（与 `agent_turn` 注入正文一致）。
/// 聊天区整体展示可由 `message_display::SHOW_STAGED_STEP_USER_BOILERPLATE_IN_CHAT` 隐藏整段注入正文；`Message.content` 与日志仍保留全文。
pub const STAGED_STEP_USER_BOILERPLATE: &str =
    "请只专注完成下列规划步骤，本步完成后以非 tool_calls 的终答结束；不要提前执行后续步骤。";

/// 两轮 NL 展示（`staged_plan_two_phase_nl_display`）桥接 **user** 正文首行；与分步注入一致，**展示层整段隐藏**（`message_display` / 前端 `message_format`），仅模型与持久化可见。
pub const STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX: &str = "### CrabMate·NL补全\n";
