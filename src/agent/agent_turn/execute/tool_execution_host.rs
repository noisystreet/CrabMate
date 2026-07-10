//! 根包 [`ToolExecutionHost`] 实现（`tool_registry` + `workflow_execute`）。

use std::sync::Arc;

use async_trait::async_trait;

use crate::agent::agent_turn::execute::tool_execution_trait::{
    ParallelPrefetchFailures, ParallelPrefetchParams, ToolExecutionHost,
};
use crabmate_internal::tool_registry::{self, DispatchToolParams, HandlerId, dispatch_tool};

use crate::agent::per_coord::PerCoordinator;
use crate::agent::workflow_tool_dispatch;
use crate::request_chrome_trace::RequestTurnTrace;

async fn prefetch_parallel_approval_failures_impl(
    params: ParallelPrefetchParams<'_>,
) -> ParallelPrefetchFailures {
    let ParallelPrefetchParams {
        tool_calls,
        cfg,
        web_tool_ctx,
        cli_tool_ctx,
        handler_lookup,
    } = params;
    let mut prefetch_failures = ParallelPrefetchFailures::new();
    if tool_calls.iter().any(|t| t.function.name == "http_fetch") {
        prefetch_failures.extend(
            tool_registry::prefetch_http_fetch_parallel_approvals(
                tool_calls,
                cfg,
                web_tool_ctx,
                cli_tool_ctx,
            )
            .await,
        );
    }
    prefetch_failures.extend(
        tool_registry::prefetch_parallel_syncdefault_approvals(
            tool_calls,
            web_tool_ctx,
            cli_tool_ctx,
            handler_lookup,
        )
        .await,
    );
    prefetch_failures
}

/// 进程内默认工具执行宿主（串行批 `dispatch_tool` / `workflow_execute`）。
pub struct CrabmateToolExecutionHost<'a> {
    pub per_coord: &'a mut PerCoordinator,
    pub request_chrome_trace: Option<Arc<RequestTurnTrace>>,
}

/// 并行只读批专用宿主（无 `workflow_execute`；每任务独立实例，无共享可变状态）。
#[derive(Debug, Clone, Copy, Default)]
pub struct CrabmateParallelToolDispatch;

#[async_trait]
impl ToolExecutionHost for CrabmateToolExecutionHost<'_> {
    async fn dispatch_tool_call(
        &mut self,
        name: &str,
        p: DispatchToolParams<'_>,
    ) -> (String, Option<serde_json::Value>) {
        if p.handler_lookup.id_for(name) == HandlerId::Workflow {
            workflow_tool_dispatch::dispatch_workflow_execute_tool(
                p.runtime,
                self.per_coord,
                p.cfg,
                p.effective_working_dir,
                p.workspace_is_set,
                p.args,
                self.request_chrome_trace.clone(),
            )
            .await
        } else {
            dispatch_tool(p).await
        }
    }

    async fn prefetch_parallel_approval_failures(
        &self,
        params: ParallelPrefetchParams<'_>,
    ) -> ParallelPrefetchFailures {
        prefetch_parallel_approval_failures_impl(params).await
    }
}

#[async_trait]
impl ToolExecutionHost for CrabmateParallelToolDispatch {
    async fn dispatch_tool_call(
        &mut self,
        name: &str,
        p: DispatchToolParams<'_>,
    ) -> (String, Option<serde_json::Value>) {
        if p.handler_lookup.id_for(name) == HandlerId::Workflow {
            return (
                "错误：并行只读批不支持 workflow_execute。".to_string(),
                None,
            );
        }
        dispatch_tool(p).await
    }

    async fn prefetch_parallel_approval_failures(
        &self,
        params: ParallelPrefetchParams<'_>,
    ) -> ParallelPrefetchFailures {
        prefetch_parallel_approval_failures_impl(params).await
    }
}

/// 并行只读批内 `http_fetch`（`spawn_blocking` + `run_direct`）入参。
pub struct ParallelHttpFetchParams<'a> {
    pub cfg: &'a Arc<crate::config::AgentConfig>,
    pub args: &'a str,
    pub effective_working_dir: &'a std::path::Path,
    pub read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    pub workspace_changelist: Option<Arc<crate::workspace::changelist::WorkspaceChangelist>>,
    pub long_term_memory: Option<Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    pub long_term_memory_scope_id: Option<String>,
}

impl CrabmateParallelToolDispatch {
    /// 并行只读批：`http_fetch` 经阻塞池直调（审批已在批前 prefetch）。
    pub async fn dispatch_parallel_http_fetch(params: ParallelHttpFetchParams<'_>) -> String {
        let ParallelHttpFetchParams {
            cfg,
            args,
            effective_working_dir,
            read_file_turn_cache,
            workspace_changelist,
            long_term_memory,
            long_term_memory_scope_id,
        } = params;
        let cfg = Arc::clone(cfg);
        let args = args.to_string();
        let wd = effective_working_dir.to_path_buf();
        let rfc = read_file_turn_cache;
        let wcl = workspace_changelist;
        let ltm = long_term_memory;
        let ltm_scope = long_term_memory_scope_id;
        let span_http = tracing::Span::current();
        tokio::task::spawn_blocking(move || {
            let _g = span_http.enter();
            let hosts = crate::memory_tool_hosts::DispatchMemoryHosts::from_dispatch_inputs(
                cfg.as_ref(),
                ltm,
                ltm_scope.as_deref(),
            );
            let ctx = crate::tools::tool_context_for_with_read_cache_and_memory(
                cfg.as_ref(),
                cfg.command_exec.allowed_commands.as_ref(),
                wd.as_path(),
                rfc.as_ref().map(|a| a.as_ref()),
                wcl.as_ref(),
                Some(hosts.codebase_ref()),
                hosts.long_term_ref(),
            );
            crate::tools::http_fetch::run_direct(&args, &ctx)
        })
        .await
        .unwrap_or_else(|e| format!("工具执行 panic：{}", e))
    }
}

/// 仅 `tool_registry::dispatch_tool`（无 `workflow_execute`）；供分层 Operator 等路径复用。
pub use CrabmateParallelToolDispatch as CrabmateRegistryToolDispatch;
