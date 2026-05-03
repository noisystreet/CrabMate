//! 单进程内共享的运行时句柄（非 `static`）：工作区变更集注册表、工具调用统计记录器与 CLI 长期记忆缓存。
//! 由 Web `AppState` 或 CLI 入口构造并注入 [`crate::RunAgentTurnParams`]，避免隐式全局状态。

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::tool_stats::ToolOutcomeRecorder;
use crate::workspace::changelist::WorkspaceChangelistRegistry;

/// Web `serve` 与 CLI `chat`/`repl` 共用的进程级句柄（显式 `Arc` 传递，替代模块级 `static`）。
pub struct ProcessHandles {
    pub workspace_changelist_registry: Arc<WorkspaceChangelistRegistry>,
    pub tool_outcome_recorder: Arc<ToolOutcomeRecorder>,
    /// CLI：懒打开的长期记忆运行时（路径变更后下次调用会重开）。
    cli_long_term_memory: Mutex<Option<(PathBuf, Arc<LongTermMemoryRuntime>)>>,
}

impl ProcessHandles {
    pub fn new(
        workspace_changelist_registry: Arc<WorkspaceChangelistRegistry>,
        tool_outcome_recorder: Arc<ToolOutcomeRecorder>,
    ) -> Self {
        Self {
            workspace_changelist_registry,
            tool_outcome_recorder,
            cli_long_term_memory: Mutex::new(None),
        }
    }

    pub fn new_arc(
        workspace_changelist_registry: Arc<WorkspaceChangelistRegistry>,
        tool_outcome_recorder: Arc<ToolOutcomeRecorder>,
    ) -> Arc<Self> {
        Arc::new(Self::new(
            workspace_changelist_registry,
            tool_outcome_recorder,
        ))
    }

    /// `bench` 等未显式传入句柄时的回退：单例（仅用于无 Web `AppState` 的路径）。
    pub fn singleton_for_fallback_process() -> Arc<Self> {
        static HANDLES: OnceLock<Arc<ProcessHandles>> = OnceLock::new();
        HANDLES
            .get_or_init(|| {
                ProcessHandles::new_arc(
                    Arc::new(WorkspaceChangelistRegistry::default()),
                    Arc::new(ToolOutcomeRecorder::new()),
                )
            })
            .clone()
    }

    pub(crate) fn cli_long_term_memory_handles_with_stderr_notice(
        self: &Arc<Self>,
        cfg: &crate::config::AgentConfig,
        failure_notified: &std::sync::atomic::AtomicBool,
    ) -> (Option<Arc<LongTermMemoryRuntime>>, Option<String>) {
        Self::cli_long_term_memory_handles_inner(self, cfg, Some(failure_notified))
    }

    fn cli_long_term_memory_handles_inner(
        self: &Arc<Self>,
        cfg: &crate::config::AgentConfig,
        failure_notified: Option<&std::sync::atomic::AtomicBool>,
    ) -> (Option<Arc<LongTermMemoryRuntime>>, Option<String>) {
        if !cfg.long_term_memory_enabled {
            return (None, None);
        }
        let path = {
            let p = cfg.long_term_memory_store_sqlite_path.trim();
            if p.is_empty() {
                std::path::Path::new(&cfg.run_command_working_dir)
                    .join(".crabmate")
                    .join("long_term_memory.db")
            } else {
                std::path::PathBuf::from(p)
            }
        };
        let mut guard = self
            .cli_long_term_memory
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some((stored, rt)) = guard.as_ref()
            && stored == &path
        {
            return (Some(Arc::clone(rt)), Some("cli".to_string()));
        }
        match LongTermMemoryRuntime::open(&path) {
            Ok(r) => {
                let a = Arc::clone(&r);
                *guard = Some((path, r));
                (Some(a), Some("cli".to_string()))
            }
            Err(e) => {
                log::warn!(
                    target: "crabmate",
                    "CLI 长期记忆库打开失败 path={} error={}",
                    path.display(),
                    e
                );
                if let Some(flag) = failure_notified
                    && !flag.swap(true, std::sync::atomic::Ordering::SeqCst)
                {
                    let detail = e.to_string();
                    let max = 240usize;
                    let (head, tail) = if detail.chars().count() > max {
                        let head: String = detail.chars().take(max).collect();
                        (head, "…")
                    } else {
                        (detail, "")
                    };
                    eprintln!(
                        "crabmate: 警告：配置中已启用长期记忆 (long_term_memory_enabled)，但本进程无法打开 SQLite；长期记忆在本进程中已禁用。\n\
                         路径: {}\n\
                         错误: {}{}\n\
                         请检查目录权限、磁盘空间或向量后端依赖（如 fastembed / ONNX）；若暂不需要可设 long_term_memory_enabled = false。详情见日志 (target=crabmate)。",
                        path.display(),
                        head,
                        tail
                    );
                }
                (None, None)
            }
        }
    }
}
