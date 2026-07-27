//! 终端等待指示器（旋转菊）的统一入口。
//!
//! 仅在 **CLI plain 终端流**（[`try_start_for_cli_plain_stream`] 的 `cli_terminal_plain`）下启动；
//! **TUI / Web**（`plain_terminal_stream = false`）不得写 stderr，以免叠到 ratatui 底栏。
//!
//! 另须 **`CM_CLI_WAIT_SPINNER`** 为真、stderr 为 TTY、未设 **`NO_COLOR`**（与文档一致，默认关）。

use std::io::{self, IsTerminal};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

static SPINNER_DISABLED: AtomicBool = AtomicBool::new(false);

/// 在 `HEADLESS` 或非终端环境下禁用——由 `cli_doctor` / benchmark runner 等提前调用。
pub fn disable_spinner_globally() {
    SPINNER_DISABLED.store(true, Ordering::Relaxed);
}

/// 结束已在运行的旋转指示器（不限于 `CliWaitSpinnerGuard` 作用域内的开始次数）。
#[allow(clippy::collapsible_if)]
pub fn finish_cli_wait_spinner() {
    if let Ok(guard) = GLOBAL_SPINNER.lock() {
        if let Some(ref inner) = *guard {
            inner.finish_and_clear();
        }
    }
    GLOBAL_PROGRESS_HIDDEN.store(true, Ordering::Relaxed);
}

static GLOBAL_SPINNER: LazyLock<std::sync::Mutex<Option<ProgressBar>>> =
    LazyLock::new(|| std::sync::Mutex::new(None));
static GLOBAL_PROGRESS_HIDDEN: AtomicBool = AtomicBool::new(false);

fn env_truthy(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| {
        let s = v.to_string_lossy();
        let s = s.trim();
        !s.is_empty() && s != "0" && !s.eq_ignore_ascii_case("false")
    })
}

/// 是否允许启动 CLI wait spinner（不含「已有实例 / 已全局隐藏」）。
#[must_use]
pub fn cli_wait_spinner_should_start(cli_terminal_plain: bool) -> bool {
    if !cli_terminal_plain || SPINNER_DISABLED.load(Ordering::Relaxed) {
        return false;
    }
    if GLOBAL_PROGRESS_HIDDEN.load(Ordering::Relaxed) {
        return false;
    }
    std::env::var_os("NO_COLOR").is_none()
        && io::stderr().is_terminal()
        && env_truthy("CM_CLI_WAIT_SPINNER")
}

/// 旋转菊守卫：构造时创建全局旋转菊；`Drop` 时移除。
pub struct CliWaitSpinnerGuard;

impl CliWaitSpinnerGuard {
    /// 仅在 **CLI plain 终端流**下创建并记录全局进度条；TUI（`cli_terminal_plain = false`）跳过。
    pub fn try_start_for_cli_plain_stream(cli_terminal_plain: bool) -> Option<Self> {
        if !cli_wait_spinner_should_start(cli_terminal_plain) {
            return None;
        }
        let spinner = ProgressBar::new_spinner();
        spinner.set_draw_target(ProgressDrawTarget::stderr());
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner} {msg}")
                .expect("spinner template"),
        );
        spinner.set_message("等待模型响应…");
        spinner.enable_steady_tick(std::time::Duration::from_millis(100));

        let mut guard = GLOBAL_SPINNER.lock().expect("spinner lock");
        if guard.is_some() {
            return None;
        }
        *guard = Some(spinner);
        Some(Self)
    }
}

impl Drop for CliWaitSpinnerGuard {
    fn drop(&mut self) {
        finish_cli_wait_spinner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_or_non_plain_never_starts_spinner() {
        // TUI / Web：`cli_terminal_plain == false`；无论 env，都不得启动（避免叠底栏）。
        assert!(!cli_wait_spinner_should_start(false));
    }
}
