//! Dynamic summary **functions** for `ToolSpec::summary`'s `dynamic by` arm.
//!
//! 绝大多数动态摘要已由 `impl ToolArgsSummary for XArgs` 从 Args 类型直接派生
//! （见 `super::tool_summary_args`）；只有「两个工具共用同一个 Args 类型、摘要要带各自动词」
//! 这一种情况才需要这里的具名函数。新增工具请优先走 Args 同源，不要往这里加壳函数。

/// `copy_file` / `move_file` 共用 `FileFromToOverwriteArgs`：字段一致、只有动词不同，
/// 摘要文本无法由 Args 类型单独决定，所以走 `specs` 的 `summary: dynamic by <fn>`。
/// 实现仍与 Args 同侧，见 `tool_summary_args`。
pub(super) use super::tool_summary_args::{summary_copy_file, summary_move_file};
