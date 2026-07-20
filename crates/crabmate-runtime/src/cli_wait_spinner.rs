//! 终端等待指示器（旋转菊 / 进度条）的统一入口。
//!
//! 当 LLM 请求处于纯 Text 流模式（`stream: false`）且终端为交互式（非管道/非 TUI）时启动。

use indicatif::{ProgressBar, ProgressStyle};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

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

// ---------------------------------------------------------------------------
// 全局旋转菊守卫
// ---------------------------------------------------------------------------

static GLOBAL_SPINNER: LazyLock<std::sync::Mutex<Option<ProgressBar>>> =
    LazyLock::new(|| std::sync::Mutex::new(None));
static GLOBAL_PROGRESS_HIDDEN: AtomicBool = AtomicBool::new(false);

/// 旋转菊守卫：构造时创建全局旋转菊；`Drop` 时移除。
pub struct CliWaitSpinnerGuard;

impl CliWaitSpinnerGuard {
    /// 仅在纯 CLI 风格下创建并记录全局进度条，若已存在则复用。
    /// `is_tui_session`：当前是否处于 TUI 会话；TUI 下跳过终端 spinner。
    pub fn try_start_for_cli_plain_stream(is_tui_session: bool) -> Option<Self> {
        if is_tui_session || SPINNER_DISABLED.load(Ordering::Relaxed) {
            return None;
        }
        // 已全局隐藏（如前一回合已 finish）则不启动新花
        if GLOBAL_PROGRESS_HIDDEN.load(Ordering::Relaxed) {
            return None;
        }
        let spinner = ProgressBar::new_spinner();
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
            // 已有 spinner 运行中，跳过
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
