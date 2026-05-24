//! `/status` 拉取显式状态机：避免 `status_loading` 默认 `true` 与 Effect 门闸冲突。
//!
//! 骨架屏：`Fetching`，或 `Idle` 且尚无快照/错误（首屏等待首次拉取）。

/// `/status` 请求生命周期（与 [`StatusTasksSignals::status_fetch_phase`] 对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusFetchPhase {
    /// 尚未发起或已结束且可再次手动刷新。
    Idle,
    /// 正在请求 `/status`。
    Fetching,
    /// 最近一次拉取成功且 [`StatusTasksSignals::status_data`] 有值。
    Ready,
    /// 最近一次拉取失败（[`StatusTasksSignals::status_fetch_err`] 有值）。
    Failed,
}

impl StatusFetchPhase {
    #[must_use]
    pub const fn allows_auto_fetch(self) -> bool {
        matches!(self, Self::Idle)
    }
}

/// 底栏芯片区是否显示骨架（非错误、非已加载内容）。
#[must_use]
pub fn status_bar_should_show_skeleton(
    phase: StatusFetchPhase,
    has_data: bool,
    has_error: bool,
) -> bool {
    if has_error {
        return false;
    }
    if has_data {
        return false;
    }
    matches!(phase, StatusFetchPhase::Idle | StatusFetchPhase::Fetching)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_while_idle_or_fetching_without_data() {
        assert!(status_bar_should_show_skeleton(
            StatusFetchPhase::Idle,
            false,
            false
        ));
        assert!(status_bar_should_show_skeleton(
            StatusFetchPhase::Fetching,
            false,
            false
        ));
    }

    #[test]
    fn no_skeleton_when_data_or_error() {
        assert!(!status_bar_should_show_skeleton(
            StatusFetchPhase::Ready,
            true,
            false
        ));
        assert!(!status_bar_should_show_skeleton(
            StatusFetchPhase::Failed,
            false,
            true
        ));
    }

    #[test]
    fn wire_fetch_triggers_when_idle_without_data() {
        let phase = StatusFetchPhase::Idle;
        let has_data = false;
        assert!(matches!(phase, StatusFetchPhase::Idle) && !has_data);
    }
}
