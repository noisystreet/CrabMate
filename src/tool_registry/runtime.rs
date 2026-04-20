//! Web / CLI 工具运行时上下文（审批通道、白名单、CLI 统计）。

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::{Mutex as TokioMutex, mpsc};

use crate::types::CommandApprovalDecision;

// --- 运行时上下文 ---

pub enum ToolRuntime<'a> {
    Web {
        workspace_changed: &'a mut bool,
        /// 仅 Web 流式会话在启用审批时提供；普通 `/chat` 或旧客户端为 `None`。
        ctx: Option<&'a WebToolRuntime>,
    },
    /// 终端 CLI：`run_command` 非白名单时走 stdin 确认（与 Web 审批语义一致）。
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

impl WebToolRuntime {
    pub(crate) fn approval_sink(&self) -> crate::tool_approval::WebApprovalSink<'_> {
        crate::tool_approval::WebApprovalSink {
            out_tx: &self.out_tx,
            approval_rx_shared: &self.approval_rx_shared,
            approval_request_guard: &self.approval_request_guard,
        }
    }
}

/// CLI 统计：用于 `chat` 退出码（本进程内 `run_command` 调用次数与用户拒绝次数）。
#[derive(Debug, Default, Clone, Copy)]
pub struct CliCommandTurnStats {
    pub run_command_attempts: u32,
    pub run_command_denials: u32,
}

/// CLI REPL / 单次提问：对**不在** `allowed_commands` 的 `run_command` 在终端 stdin 交互确认；**永久允许**写入本结构（进程内）。
#[derive(Clone)]
pub struct CliToolRuntime {
    pub persistent_allowlist_shared: Arc<TokioMutex<HashSet<String>>>,
    /// `--yes`：对 [`crate::tool_approval::SensitiveCapability`] 所覆盖的敏感工具（`run_command`、未匹配前缀的 `http_fetch` / `http_request` 等）在非白名单时也自动「本次允许」（**仅可信环境**；与 [`crate::tool_approval::CliApprovalInput`] 同源语义）。
    pub auto_approve_all_non_whitelist_run_command: bool,
    /// `--approve-commands` 额外允许的命令名（小写），与配置白名单合并后再决定是否提示。
    pub extra_allowlist_commands: Arc<[String]>,
    pub command_stats: Arc<std::sync::Mutex<CliCommandTurnStats>>,
}

impl CliToolRuntime {
    /// REPL / 默认单次问答：交互审批，不自动批准。
    pub fn new_interactive_default() -> Self {
        Self {
            persistent_allowlist_shared: Arc::new(TokioMutex::new(HashSet::new())),
            auto_approve_all_non_whitelist_run_command: false,
            extra_allowlist_commands: Arc::from([] as [String; 0]),
            command_stats: Arc::new(std::sync::Mutex::new(CliCommandTurnStats::default())),
        }
    }

    pub fn reset_command_stats(&self) {
        if let Ok(mut g) = self.command_stats.lock() {
            *g = CliCommandTurnStats::default();
        }
    }

    pub(crate) fn record_run_command_attempt(&self) {
        if let Ok(mut g) = self.command_stats.lock() {
            g.run_command_attempts = g.run_command_attempts.saturating_add(1);
        }
    }

    pub(crate) fn record_run_command_denial(&self) {
        if let Ok(mut g) = self.command_stats.lock() {
            g.run_command_denials = g.run_command_denials.saturating_add(1);
        }
    }

    /// 本回合（自上次 [`Self::reset_command_stats`]）内每次 `run_command` 均被用户拒绝。
    pub fn all_run_commands_were_denied(&self) -> bool {
        self.command_stats.lock().is_ok_and(|g| {
            g.run_command_attempts > 0 && g.run_command_denials == g.run_command_attempts
        })
    }
}
