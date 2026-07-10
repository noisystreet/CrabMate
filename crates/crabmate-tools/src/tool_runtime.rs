//! Web / CLI 工具运行时上下文（审批通道、白名单、CLI 统计）。

use std::collections::HashSet;
use std::sync::Arc;

use crabmate_types::CommandApprovalDecision;
use tokio::sync::{Mutex as TokioMutex, mpsc};

/// 全屏 TUI：工具审批由 UI 线程接管 stdin，异步侧在 [`Self::respond_tx`] 上阻塞等待结果。
#[derive(Debug)]
pub struct TuiApprovalRequest {
    pub title: String,
    pub detail: String,
    pub respond_tx: std::sync::mpsc::Sender<CommandApprovalDecision>,
}

pub enum ToolRuntime<'a> {
    Web {
        workspace_changed: &'a mut bool,
        ctx: Option<&'a WebToolRuntime>,
    },
    Cli {
        workspace_changed: &'a mut bool,
        ctx: &'a CliToolRuntime,
    },
}

pub struct WebToolRuntime {
    pub out_tx: mpsc::Sender<String>,
    pub approval_rx_shared: Arc<TokioMutex<mpsc::Receiver<CommandApprovalDecision>>>,
    pub approval_request_guard: Arc<TokioMutex<()>>,
    pub persistent_allowlist_shared: Arc<TokioMutex<HashSet<String>>>,
}

/// CLI 统计：用于 `chat` 退出码（本进程内 `run_command` 调用次数与用户拒绝次数）。
#[derive(Debug, Default, Clone, Copy)]
pub struct CliCommandTurnStats {
    pub run_command_attempts: u32,
    pub run_command_denials: u32,
}

/// CLI REPL / 单次提问：`run_command` 非白名单时在终端 stdin 交互确认。
#[derive(Clone)]
pub struct CliToolRuntime {
    pub persistent_allowlist_shared: Arc<TokioMutex<HashSet<String>>>,
    pub auto_approve_all_non_whitelist_run_command: bool,
    pub extra_allowlist_commands: Arc<[String]>,
    pub command_stats: Arc<std::sync::Mutex<CliCommandTurnStats>>,
    pub tui_blocking_approval_tx: Option<std::sync::mpsc::SyncSender<TuiApprovalRequest>>,
}

impl CliToolRuntime {
    pub fn new_interactive_default() -> Self {
        Self {
            persistent_allowlist_shared: Arc::new(TokioMutex::new(HashSet::new())),
            auto_approve_all_non_whitelist_run_command: false,
            extra_allowlist_commands: Arc::from([] as [String; 0]),
            command_stats: Arc::new(std::sync::Mutex::new(CliCommandTurnStats::default())),
            tui_blocking_approval_tx: None,
        }
    }

    pub fn with_tui_blocking_approval(
        mut self,
        tx: std::sync::mpsc::SyncSender<TuiApprovalRequest>,
    ) -> Self {
        self.tui_blocking_approval_tx = Some(tx);
        self
    }

    pub fn reset_command_stats(&self) {
        if let Ok(mut g) = self.command_stats.lock() {
            *g = CliCommandTurnStats::default();
        }
    }

    pub fn record_run_command_attempt(&self) {
        if let Ok(mut g) = self.command_stats.lock() {
            g.run_command_attempts = g.run_command_attempts.saturating_add(1);
        }
    }

    pub fn record_run_command_denial(&self) {
        if let Ok(mut g) = self.command_stats.lock() {
            g.run_command_denials = g.run_command_denials.saturating_add(1);
        }
    }

    pub fn all_run_commands_were_denied(&self) -> bool {
        self.command_stats.lock().is_ok_and(|g| {
            g.run_command_attempts > 0 && g.run_command_denials == g.run_command_attempts
        })
    }
}
