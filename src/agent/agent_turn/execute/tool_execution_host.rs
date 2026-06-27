//! 根包 [`crabmate_agent::ToolExecutionHost`] 实现（`tool_registry` + `workflow_execute`）。

use std::sync::Arc;

use async_trait::async_trait;

use crabmate_agent::agent_turn::ToolExecutionHost;
use crabmate_internal::tool_registry::{DispatchToolParams, HandlerId, dispatch_tool};

use crate::agent::per_coord::PerCoordinator;
use crate::agent::workflow_tool_dispatch;
use crate::request_chrome_trace::RequestTurnTrace;

/// 进程内默认工具执行宿主（串行批 `dispatch_tool` / `workflow_execute`）。
pub struct CrabmateToolExecutionHost<'a> {
    pub per_coord: &'a mut PerCoordinator,
    pub request_chrome_trace: Option<Arc<RequestTurnTrace>>,
}

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
}
