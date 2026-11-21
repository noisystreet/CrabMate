//! 单会话自动续跑的**进程内**账本（`#1455` C 步）。
//!
//! ## 为什么必须进程内
//!
//! 轮数硬闸（`autonomy_max_autonomous_turns`）的真源**不能**是
//! `.crabmate/tasks/<scope>.json`——那份文件可手删，删掉即可无限续跑。这与累计预算
//! 那把**软闸**是相反的性质（见 `cm_agent::task_state` 的判据 4：闸门强度不能超过真源可靠性）。
//!
//! **重启归零是期望行为**：重新计数比持久化更安全，也更简单。
//!
//! ## 另一个职责：无进展检测
//!
//! 轮数硬闸只保证「最多 N 轮」。但如果续跑后**立刻因同一原因**再次暂停，那 N 轮会在几秒内
//! 烧完且没有任何进展。本模块因此记录「连续同因暂停」次数，达到阈值即停——这是通用的
//! 重试无进展检测，将来 webhook 重试、定时任务失败重试都能复用。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::cm_agent::task_state::TaskPauseReason;

/// 连续「同因暂停」达到该次数即判定无进展、停止续跑。
///
/// 取 `2`：一次同因暂停可能只是刚好卡在边界（例如累计 token 差一点点），
/// 连续两次则说明续跑没有换到新状态。
pub const MAX_CONSECUTIVE_SAME_REASON: u32 = 2;

/// 条目存活时长：超过则在下一次 [`RunEpoch::prune`] 丢弃（防长跑进程缓慢泄漏）。
pub const EPOCH_TTL: Duration = Duration::from_secs(6 * 3600);

/// 条目数上限（兜底：scope 高度分散的部署，例如每次请求都新建会话）。
pub const MAX_EPOCH_ENTRIES: usize = 1024;

#[derive(Debug, Clone, Copy)]
struct EpochEntry {
    resumed_turns: u32,
    last_pause_reason: Option<TaskPauseReason>,
    consecutive_same_reason: u32,
    last_touched: Instant,
}

/// 某会话当前可用于决策的快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EpochSnapshot {
    /// 本纪元内已自动续跑的轮数。
    pub resumed_turns: u32,
    /// 连续因**同一原因**暂停的次数（正常完成会归零）。
    pub consecutive_same_reason: u32,
    /// 最近一次暂停原因（排障用）。
    pub last_pause_reason: Option<TaskPauseReason>,
}

/// 进程内的续跑账本。
///
/// 无内部同步：调用方（`AppState`）用 `Mutex` 包住即可，与既有缓存字段同模式。
#[derive(Debug, Default)]
pub struct RunEpoch {
    entries: HashMap<String, EpochEntry>,
}

impl RunEpoch {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 顺从这次 `prune`，返回被丢弃的条目数。
    ///
    /// 两段清理：先按 TTL，再按数量（保留最近活动的）。**顺序不可反**——
    /// 先按数量删可能把「刚过期但很新」的留下，而 TTL 才是真正该先判的语义。
    pub fn prune(&mut self, now: Instant) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|_, e| now.saturating_duration_since(e.last_touched) < EPOCH_TTL);
        if self.entries.len() > MAX_EPOCH_ENTRIES {
            let mut ages: Vec<(String, Instant)> = self
                .entries
                .iter()
                .map(|(k, e)| (k.clone(), e.last_touched))
                .collect();
            ages.sort_by_key(|(_, t)| *t);
            let drop_n = self.entries.len() - MAX_EPOCH_ENTRIES;
            for (k, _) in ages.into_iter().take(drop_n) {
                self.entries.remove(&k);
            }
        }
        before - self.entries.len()
    }

    fn entry_mut(&mut self, scope: &str, now: Instant) -> &mut EpochEntry {
        self.entries.entry(scope.to_string()).or_insert(EpochEntry {
            resumed_turns: 0,
            last_pause_reason: None,
            consecutive_same_reason: 0,
            last_touched: now,
        })
    }

    /// 一次自动续跑**开始**时记账（轮数 +1）。顺带做一次轻量清理。
    pub fn note_resume(&mut self, scope: &str) {
        let now = Instant::now();
        self.prune(now);
        let e = self.entry_mut(scope, now);
        e.resumed_turns = e.resumed_turns.saturating_add(1);
        e.last_touched = now;
    }

    /// 记录本回合**因预算暂停**。
    ///
    /// `from_auto_resume` 决定「连续同因」是否计入——这是本方法的语义核心：
    ///
    /// - `false`（用户发起）：**清零**。用户手动重试本身就是「情况变了」的信号，
    ///   把它算进「续跑无进展」会让第 3 次手动重试就被 `NoProgress` 拦下。
    /// - `true`（自动续跑）：与上次同因则累加，换了原因则从 1 重新计。
    pub fn record_pause(&mut self, scope: &str, reason: TaskPauseReason, from_auto_resume: bool) {
        let now = Instant::now();
        let e = self.entry_mut(scope, now);
        e.consecutive_same_reason = if !from_auto_resume {
            0
        } else if e.last_pause_reason == Some(reason) {
            e.consecutive_same_reason.saturating_add(1)
        } else {
            1
        };
        e.last_pause_reason = Some(reason);
        e.last_touched = now;
    }

    /// 本回合**有进展**（正常完成，或非预算中止）：连续同因计数归零。
    ///
    /// 只清连续计数、**不清** `resumed_turns`：轮数是本纪元的累计硬闸，不该因一次成功而回退。
    pub fn clear_progress(&mut self, scope: &str) {
        let now = Instant::now();
        if let Some(e) = self.entries.get_mut(scope) {
            e.consecutive_same_reason = 0;
            e.last_pause_reason = None;
            e.last_touched = now;
        }
    }

    #[must_use]
    pub fn snapshot(&self, scope: &str) -> EpochSnapshot {
        self.entries
            .get(scope)
            .map(|e| EpochSnapshot {
                resumed_turns: e.resumed_turns,
                consecutive_same_reason: e.consecutive_same_reason,
                last_pause_reason: e.last_pause_reason,
            })
            .unwrap_or_default()
    }

    /// 显式遗忘（会话结束 / 用户手动接管时调用）。
    pub fn forget(&mut self, scope: &str) -> bool {
        self.entries.remove(scope).is_some()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_scope_is_default() {
        let e = RunEpoch::new();
        assert_eq!(e.snapshot("nope"), EpochSnapshot::default());
    }

    #[test]
    fn resume_count_accumulates_and_is_not_reset_by_progress() {
        let mut e = RunEpoch::new();
        e.note_resume("s");
        e.note_resume("s");
        assert_eq!(e.snapshot("s").resumed_turns, 2);
        e.clear_progress("s");
        assert_eq!(
            e.snapshot("s").resumed_turns,
            2,
            "轮数是纪元累计，不该因一次成功回退"
        );
    }

    #[test]
    fn consecutive_same_reason_counts_only_when_repeating() {
        let mut e = RunEpoch::new();
        e.record_pause("s", TaskPauseReason::BudgetTokens, true);
        assert_eq!(e.snapshot("s").consecutive_same_reason, 1);
        e.record_pause("s", TaskPauseReason::BudgetTokens, true);
        assert_eq!(e.snapshot("s").consecutive_same_reason, 2);
        // 换原因 → 重新从 1 开始
        e.record_pause("s", TaskPauseReason::BudgetLlmCalls, true);
        assert_eq!(e.snapshot("s").consecutive_same_reason, 1);
    }

    /// 用户手动重试**不该**计入「续跑无进展」——否则第 3 次手动重试就被 NoProgress 拦下。
    ///
    /// 判据：`consecutive_same_reason` 统计的是「自动续跑后仍然同因」，
    /// 而用户手动重发本身就是「情况变了」的信号。
    #[test]
    fn user_initiated_pause_resets_consecutive() {
        let mut e = RunEpoch::new();
        e.record_pause("s", TaskPauseReason::BudgetTokens, true);
        e.record_pause("s", TaskPauseReason::BudgetTokens, true);
        assert_eq!(e.snapshot("s").consecutive_same_reason, 2);

        // 用户手动再发又失败：清零。
        e.record_pause("s", TaskPauseReason::BudgetTokens, false);
        assert_eq!(e.snapshot("s").consecutive_same_reason, 0);
    }

    #[test]
    fn progress_resets_consecutive_but_keeps_last_reason_cleared() {
        let mut e = RunEpoch::new();
        e.record_pause("s", TaskPauseReason::BudgetWallClock, true);
        e.record_pause("s", TaskPauseReason::BudgetWallClock, true);
        e.clear_progress("s");
        let snap = e.snapshot("s");
        assert_eq!(snap.consecutive_same_reason, 0);
        assert_eq!(snap.last_pause_reason, None);
    }

    #[test]
    fn forget_removes_entry() {
        let mut e = RunEpoch::new();
        e.note_resume("s");
        assert!(e.forget("s"));
        assert!(!e.forget("s"), "重复遗忘应返回 false");
        assert_eq!(e.len(), 0);
    }

    /// TTL 清理：过期条目被丢，新条目留下。
    #[test]
    fn prune_drops_expired_by_ttl() {
        let mut e = RunEpoch::new();
        e.note_resume("old");
        // 把 last_touched 挪到过去：直接构造过期时刻不可行，故按 TTL 反推 now。
        let future = Instant::now() + EPOCH_TTL + Duration::from_secs(1);
        e.note_resume("new_after_rebase");
        let dropped = e.prune(future);
        assert!(dropped >= 1, "过期条目应被清理");
        assert_eq!(e.snapshot("old"), EpochSnapshot::default());
    }

    /// 数量上限：超出后按「最久未活动」淘汰。
    #[test]
    fn prune_enforces_max_entries_oldest_first() {
        let mut e = RunEpoch::new();
        for i in 0..(MAX_EPOCH_ENTRIES + 5) {
            e.note_resume(&format!("s{i}"));
        }
        assert!(e.len() > MAX_EPOCH_ENTRIES);
        e.prune(Instant::now());
        assert_eq!(e.len(), MAX_EPOCH_ENTRIES);
    }

    #[test]
    fn prune_is_noop_when_small_and_fresh() {
        let mut e = RunEpoch::new();
        e.note_resume("s");
        assert_eq!(e.prune(Instant::now()), 0);
        assert_eq!(e.len(), 1);
    }
}
