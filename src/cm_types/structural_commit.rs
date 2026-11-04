//! 结构提交结果：一次 `ModelContextView` 提交中**被移出 canonical** 的原文。
//!
//! 为什么放在 `protocol` 层（`cm_types`）而不是 `agent`：
//! `conversation_store` 需要把它转成归档行（`archive_rows_from_structural_commit`），
//! 而存储层不应依赖回合编排层——否则存储会被 `server` 一起拖住，无法单独复用
//! （S1-b，见 #1278）。本类型只含 `Message` 与基本类型，天然属于叶子层。

use crate::cm_types::Message;

/// 结构性提交结果：本轮被移出 canonical 的原文（归档候选）与追加条数（见 §3.7）。
#[derive(Clone, Debug, Default)]
pub struct StructuralCommit {
    /// 被移出 canonical 的连续原文前缀（逐字原文，交由调用方归档）。
    pub archived_prefix: Vec<Message>,
    /// 该前缀是否由摘要替换移出（`true` → `summarized`，否则 `removed_turn_group`）。
    pub prefix_summarized: bool,
    /// 超出「canonical 至多 1 条」上限、被回收的旧 artifact 旁注（见 §3.9）。
    pub archived_artifacts: Vec<Message>,
    /// 本次追加回 canonical 的当前回合消息条数。
    pub appended: usize,
    /// 移出区间在提交前 canonical 中的下标快照（**仅**人审 / 排查）。
    pub range_start: usize,
    pub range_end: usize,
}

impl StructuralCommit {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.archived_prefix.is_empty() && self.archived_artifacts.is_empty() && self.appended == 0
    }
}
