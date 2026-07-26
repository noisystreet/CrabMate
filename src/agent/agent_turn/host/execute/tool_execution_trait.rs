//! 根包工具执行宿主 trait（依赖 `tool_registry` 运行时类型，故置于编排层而非 `crabmate-agent`）。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use crabmate_config::AgentConfig;
use crabmate_types::ToolCall;

use crate::tool_registry::{
    CliToolRuntime, DispatchToolParams, HandlerLookupTable, WebToolRuntime,
};

/// 并行只读批预取审批失败映射的键。
pub type ParallelPrefetchFailureKey = (String, String);

/// 并行只读批预取审批失败（`name+args` → 错误正文）。
pub type ParallelPrefetchFailures = HashMap<ParallelPrefetchFailureKey, String>;

/// [`ToolExecutionHost::prefetch_parallel_approval_failures`] 入参。
pub struct ParallelPrefetchParams<'a> {
    pub tool_calls: &'a [ToolCall],
    pub cfg: &'a Arc<AgentConfig>,
    pub web_tool_ctx: Option<&'a WebToolRuntime>,
    pub cli_tool_ctx: Option<&'a CliToolRuntime>,
    pub handler_lookup: &'a HandlerLookupTable,
}

/// 根包实现的工具分发（`tool_registry::dispatch_tool` 与 `workflow_execute` 等）。
#[async_trait]
pub trait ToolExecutionHost: Send + Sync {
    async fn dispatch_tool_call(
        &mut self,
        name: &str,
        p: DispatchToolParams<'_>,
    ) -> (String, Option<serde_json::Value>);

    async fn prefetch_parallel_approval_failures(
        &self,
        params: ParallelPrefetchParams<'_>,
    ) -> ParallelPrefetchFailures;
}
