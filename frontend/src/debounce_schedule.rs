//! 侧栏「筛选会话 / 搜索消息」输入防抖的**纯代数规则**（与 `sidebar_nav` 中 `Cell<u64>` 递增语义一致）。
//!
//! 每次源信号变化时递增一代；异步等待结束后仅当「当前代数仍等于派发时的代数」才写入目标信号，否则丢弃。

/// 超时回调触发时，若代数未变，应**应用**写入。
#[inline]
pub fn debounce_should_apply(when_scheduled: u64, latest_generation: u64) -> bool {
    when_scheduled == latest_generation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_generation_applies() {
        assert!(debounce_should_apply(3, 3));
    }

    #[test]
    fn newer_generation_skips_stale_callback() {
        assert!(!debounce_should_apply(2, 5));
    }

    #[test]
    fn wrapping_u64_still_distinct_until_eq() {
        assert!(!debounce_should_apply(u64::MAX, 0)); // wrapping_add 后常见相邻代数
        assert!(debounce_should_apply(0, 0));
    }
}
