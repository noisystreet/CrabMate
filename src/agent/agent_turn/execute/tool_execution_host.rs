//! 根包 [`crabmate_agent::ToolExecutionHost`] 实现（`tool_registry` + `workflow_execute`）。

use std::sync::Arc;

use async_trait::async_trait;

use crabmate_agent::agent_turn::{
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
