//! CLI 在 **`plain_terminal_stream`** 下等待模型首包（reasoning/content delta）时，可选在 **stderr** 显示 spinner + 已等待时间（**indicatif**）。
//!
//! 与 stdout 上的 **`Agent:`** 流式正文分离，避免与 `crossterm` 行编辑冲突。启用条件：**`AGENT_CLI_WAIT_SPINNER`** 为真、**stderr 为 TTY**、未设 **`NO_COLOR`**。

use std::io::{self, IsTerminal};
use std::sync::Mutex;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

static ACTIVE_SPINNER: Mutex<Option<ProgressBar>> = Mutex::new(None);

fn env_truthy(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| {
        let s = v.to_string_lossy();
        let s = s.trim();
        !s.is_empty() && s != "0" && !s.eq_ignore_ascii_case("false")
    })
}

/// 与 REPL 其它 ANSI 约定一致：尊重 **`NO_COLOR`**；动效写在 **stderr**，故要求 stderr 为 TTY。
pub(crate) fn cli_wait_spinner_wanted() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && io::stderr().is_terminal()
        && env_truthy("AGENT_CLI_WAIT_SPINNER")
}

/// 在写出首个 plain 助手片段前调用，清除等待行，避免压在 **`Agent:`** 之上。
pub(crate) fn finish_cli_wait_spinner() {
    let mut slot = ACTIVE_SPINNER.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(pb) = slot.take() {
        pb.finish_and_clear();
    }
}

/// 在 HTTP 已成功、即将读取 SSE 或非流式 body 时挂上；[`Drop`] 时若仍在转则清除（如仅 tool_calls、无正文）。
pub(crate) struct CliWaitSpinnerGuard {
    engaged: bool,
}

impl CliWaitSpinnerGuard {
    pub(crate) fn try_start_for_cli_plain_stream(cli_terminal_plain: bool) -> Self {
        if !cli_terminal_plain || !cli_wait_spinner_wanted() {
            return Self { engaged: false };
        }
        let mut slot = ACTIVE_SPINNER.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_some() {
            return Self { engaged: false };
        }
        let pb = ProgressBar::new_spinner();
        pb.set_draw_target(ProgressDrawTarget::stderr());
        let style = ProgressStyle::with_template(
            "{spinner:.cyan.bold} [{elapsed_precise:.dim}] waiting for model …",
        )
        .unwrap_or_else(|_| ProgressStyle::default_spinner());
        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(100));
        *slot = Some(pb);
        Self { engaged: true }
    }
}

impl Drop for CliWaitSpinnerGuard {
    fn drop(&mut self) {
        if self.engaged {
            finish_cli_wait_spinner();
        }
    }
}
