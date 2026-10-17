//! `run_command` 全局速率限制（每秒最多 [`MAX_COMMANDS_PER_SEC`] 条）。
//! 作为 [`super`]（`command`）的子模块，经 `#[path]` 挂载。

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use super::RunCommandError;

/// 简单的每秒调用限流器状态
struct RateLimitState {
    window_sec: u64,
    count: u32,
}

/// 全局限流器：在任意 1 秒窗口内最多允许执行的命令数
const MAX_COMMANDS_PER_SEC: u32 = 5;

static RATE_LIMIT: Mutex<RateLimitState> = Mutex::new(RateLimitState {
    window_sec: 0,
    count: 0,
});

fn now_sec() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn check_rate_limit() -> Result<(), RunCommandError> {
    // 单元测试并行调用 `run`/`run_gh_vec` 时共享进程级计数，易误触 5/s 上限导致偶发失败。
    if cfg!(test) {
        return Ok(());
    }
    let mut state = RATE_LIMIT.lock().unwrap_or_else(|e| e.into_inner());
    let now = now_sec();
    if now != state.window_sec {
        state.window_sec = now;
        state.count = 0;
    }
    if state.count >= MAX_COMMANDS_PER_SEC {
        return Err(RunCommandError::RateLimited {
            max_per_sec: MAX_COMMANDS_PER_SEC,
        });
    }
    state.count += 1;
    Ok(())
}

/// 仅 **`cargo test`**：清空 [`RATE_LIMIT`] 窗口计数，避免与其它测试共享限流状态。
pub fn reset_run_command_rate_limit_for_tests() {
    let mut state = RATE_LIMIT.lock().unwrap_or_else(|e| e.into_inner());
    state.window_sec = 0;
    state.count = 0;
}
