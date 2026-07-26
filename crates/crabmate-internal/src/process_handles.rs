//! 单进程内共享的运行时句柄（非 `static`）：工作区变更集注册表、工具调用统计记录器、只读类 **`run_command`** 短时 TTL 缓存与 CLI 长期记忆缓存。
//! 侧栏任务清单（[`workspace::tasks_side`]）与 Web **`GET`/`POST /tasks`** 共用同一内存表。
//! 由 Web `AppState` 或 CLI 入口构造并注入回合路径；**`default_arc_process_handles`** 为无 `AppState` 时的独立默认 `Arc`（**不**用进程级 `static` 单例）。
//!
//! 回合编排只消费 [`TurnProcessHandles`]（经 [`ProcessHandles::turn_handles`]）；侧栏任务与 CLI LTM 仍挂在完整 [`ProcessHandles`] 上。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache;
use crate::tool_registry::HandlerLookupTable;
use crate::tool_sandbox::SyncDefaultSandboxBackend;
use crate::tool_stats::ToolOutcomeRecorder;
use crate::workspace::changelist::WorkspaceChangelistRegistry;
use crate::workspace::tasks_side::{TasksData, WorkspaceTasksByPath};

/// `run_agent_turn` / 工具执行所需的进程句柄面（不含侧栏任务表与 CLI LTM）。
#[derive(Clone)]
pub struct TurnProcessHandles {
    pub workspace_changelist_registry: Arc<WorkspaceChangelistRegistry>,
    pub tool_outcome_recorder: Arc<ToolOutcomeRecorder>,
    /// 工具名 → 分发 handler（原模块级 `HANDLER_MAP`）。
    pub handler_lookup: HandlerLookupTable,
    /// Docker `sync_default` 沙盒后端（原模块级 `SANDBOX_BACKEND`）。
    pub sync_default_sandbox_backend: Arc<dyn SyncDefaultSandboxBackend>,
    /// 只读类 **`run_command`** 短时 TTL 缓存（按工作区键失效；配置见 **`readonly_tool_ttl_cache_*`**）。
    pub readonly_tool_ttl_cache: Arc<ReadonlyToolTtlCache>,
}

impl TurnProcessHandles {
    /// 默认回合句柄（独立 `Arc`）：bench / 单元测试等无完整 [`ProcessHandles`] 的路径。
    pub fn default_arc() -> Arc<Self> {
        ProcessHandles::default_arc_process_handles().turn_handles_arc()
    }
}

/// Web `serve` 与 CLI `chat`/`repl` 共用的进程级句柄（显式 `Arc` 传递，替代模块级 `static`）。
pub struct ProcessHandles {
    pub workspace_changelist_registry: Arc<WorkspaceChangelistRegistry>,
    pub tool_outcome_recorder: Arc<ToolOutcomeRecorder>,
    /// 工具名 → 分发 handler（原模块级 `HANDLER_MAP`）。
    pub handler_lookup: HandlerLookupTable,
    /// Docker `sync_default` 沙盒后端（原模块级 `SANDBOX_BACKEND`）。
    pub sync_default_sandbox_backend: Arc<dyn SyncDefaultSandboxBackend>,
    /// 只读类 **`run_command`** 短时 TTL 缓存（按工作区键失效；配置见 **`readonly_tool_ttl_cache_*`**）。
    pub readonly_tool_ttl_cache: Arc<ReadonlyToolTtlCache>,
    /// 与 Web 侧栏任务同源：键为规范化工作区路径字符串。
    pub workspace_tasks_by_path: WorkspaceTasksByPath,
    /// CLI：懒打开的长期记忆运行时（路径变更后下次调用会重开）。
    cli_long_term_memory: Mutex<Option<(PathBuf, Arc<LongTermMemoryRuntime>)>>,
}

impl ProcessHandles {
    pub fn new(
        workspace_changelist_registry: Arc<WorkspaceChangelistRegistry>,
        tool_outcome_recorder: Arc<ToolOutcomeRecorder>,
        handler_lookup: HandlerLookupTable,
        sync_default_sandbox_backend: Arc<dyn SyncDefaultSandboxBackend>,
    ) -> Self {
        Self {
            workspace_changelist_registry,
            tool_outcome_recorder,
            handler_lookup,
            sync_default_sandbox_backend,
            readonly_tool_ttl_cache: Arc::new(ReadonlyToolTtlCache::new()),
            workspace_tasks_by_path: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            cli_long_term_memory: Mutex::new(None),
        }
    }

    pub fn new_arc(
        workspace_changelist_registry: Arc<WorkspaceChangelistRegistry>,
        tool_outcome_recorder: Arc<ToolOutcomeRecorder>,
        handler_lookup: HandlerLookupTable,
        sync_default_sandbox_backend: Arc<dyn SyncDefaultSandboxBackend>,
    ) -> Arc<Self> {
        Arc::new(Self::new(
            workspace_changelist_registry,
            tool_outcome_recorder,
            handler_lookup,
            sync_default_sandbox_backend,
        ))
    }

    /// 回合路径消费面：变更集注册表、工具统计、handler、沙盒、只读 TTL（不含 tasks / CLI LTM）。
    pub fn turn_handles(&self) -> TurnProcessHandles {
        TurnProcessHandles {
            workspace_changelist_registry: Arc::clone(&self.workspace_changelist_registry),
            tool_outcome_recorder: Arc::clone(&self.tool_outcome_recorder),
            handler_lookup: self.handler_lookup.clone(),
            sync_default_sandbox_backend: Arc::clone(&self.sync_default_sandbox_backend),
            readonly_tool_ttl_cache: Arc::clone(&self.readonly_tool_ttl_cache),
        }
    }

    /// [`turn_handles`] 的 `Arc` 形式，供 `run_agent_turn` 入参等使用。
    pub fn turn_handles_arc(&self) -> Arc<TurnProcessHandles> {
        Arc::new(self.turn_handles())
    }

    /// 默认进程句柄（独立 `Arc`，非全局单例）：用于装配 Web/`AppState`、CLI session 等。
    /// 回合入参请用 [`TurnProcessHandles::default_arc`] 或 [`Self::turn_handles_arc`]。
    pub fn default_arc_process_handles() -> Arc<Self> {
        ProcessHandles::new_arc(
            Arc::new(WorkspaceChangelistRegistry::default()),
            Arc::new(ToolOutcomeRecorder::new()),
            HandlerLookupTable::default_dispatch(),
            crate::tool_sandbox::default_sync_default_sandbox_backend(),
        )
    }

    pub fn cli_long_term_memory_handles_with_stderr_notice(
        self: &Arc<Self>,
        cfg: &crabmate_config::AgentConfig,
        failure_notified: &std::sync::atomic::AtomicBool,
    ) -> (Option<Arc<LongTermMemoryRuntime>>, Option<String>) {
        Self::cli_long_term_memory_handles_inner(self, cfg, Some(failure_notified))
    }

    fn cli_long_term_memory_handles_inner(
        self: &Arc<Self>,
        cfg: &crabmate_config::AgentConfig,
        failure_notified: Option<&std::sync::atomic::AtomicBool>,
    ) -> (Option<Arc<LongTermMemoryRuntime>>, Option<String>) {
        if !cfg.long_term_memory.long_term_memory_enabled {
            return (None, None);
        }
        let path = {
            let p = cfg
                .long_term_memory
                .long_term_memory_store_sqlite_path
                .trim();
            if p.is_empty() {
                std::path::Path::new(&cfg.command_exec.run_command_working_dir)
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

    /// 与 Web `GET /tasks` 同源：按工作区键读取侧栏任务（进程内存）。
    pub async fn tasks_data_for_workspace_path(self: &Arc<Self>, workspace_key: &str) -> TasksData {
        let g = self.workspace_tasks_by_path.read().await;
        g.get(workspace_key).cloned().unwrap_or_default()
    }

    /// 与 Web `GET /workspace/changelog` 同源：返回 Markdown 正文；配置关闭时返回 `Err` 说明。
    pub fn workspace_changelog_markdown_for_scope(
        self: &Arc<Self>,
        cfg: &crabmate_config::AgentConfig,
        scope: &str,
    ) -> Result<String, &'static str> {
        if !cfg
            .session_workspace_changelist
            .session_workspace_changelist_enabled
        {
            return Err("会话工作区变更集已在配置中关闭（session_workspace_changelist_enabled）");
        }
        let max_chars = cfg
            .session_workspace_changelist
            .session_workspace_changelist_max_chars;
        let cl = self
            .workspace_changelist_registry
            .changelist_for_scope(scope);
        let (_rev, body) = cl.snapshot_markdown(max_chars);
        Ok(body.unwrap_or_default())
    }
}
