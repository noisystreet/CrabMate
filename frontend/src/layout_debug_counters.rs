//! 流式布局过渡债观测计数（**仅 `debug_assertions`**；release 为空操作）。
//!
//! 对应 `agent_space/streaming-layout-convergence-plan.md` Phase A：
//! - `empty_shell_skip`：本应挂载但因空助手壳被跳过的次数（读路径过滤生效）
//! - `commentary_handoff`：I14 同文移交清空 loading / overlay 的次数
//!
//! WASM debug 构建下控制台周期性输出 `[layout_debug] …`。

#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(debug_assertions)]
static EMPTY_SHELL_SKIP: AtomicU64 = AtomicU64::new(0);
#[cfg(debug_assertions)]
static COMMENTARY_HANDOFF: AtomicU64 = AtomicU64::new(0);

#[cfg(all(debug_assertions, target_arch = "wasm32"))]
fn log_debug(msg: String) {
    web_sys::console::log_1(&msg.into());
}

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
fn log_debug(_msg: String) {}

/// 空助手壳被读路径跳过时调用（TUI 过滤）。
#[inline]
pub(crate) fn note_empty_shell_skip() {
    #[cfg(debug_assertions)]
    {
        let n = EMPTY_SHELL_SKIP.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 || n.is_multiple_of(25) {
            log_debug(format!("[layout_debug] empty_shell_skip={n}"));
        }
    }
}

/// I14 旁注/终答同文移交成功时调用。
#[inline]
pub(crate) fn note_commentary_handoff() {
    #[cfg(debug_assertions)]
    {
        let n = COMMENTARY_HANDOFF.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 || n.is_multiple_of(10) {
            log_debug(format!("[layout_debug] commentary_handoff={n}"));
        }
    }
}

#[cfg(all(test, debug_assertions))]
#[must_use]
fn snapshot() -> (u64, u64) {
    (
        EMPTY_SHELL_SKIP.load(Ordering::Relaxed),
        COMMENTARY_HANDOFF.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_increment_under_debug() {
        #[cfg(debug_assertions)]
        {
            let (a0, b0) = snapshot();
            note_empty_shell_skip();
            note_commentary_handoff();
            let (a1, b1) = snapshot();
            assert!(a1 > a0);
            assert!(b1 > b0);
        }
        #[cfg(not(debug_assertions))]
        {
            note_empty_shell_skip();
            note_commentary_handoff();
        }
    }
}
