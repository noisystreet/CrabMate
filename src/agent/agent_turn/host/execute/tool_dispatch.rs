//! 窄面工具分发：打断 `host` 对 `tool_registry::dispatch_tool` 的直接依赖。
//!
//! [`ToolExecutionHost`] 经本模块的 [`ToolDispatch`]（默认 [`InternalToolDispatch`]）调用 registry；
//! 测试可注入假实现而不走真实 `dispatch_tool`。

use async_trait::async_trait;
use crabmate_internal::tool_registry::{DispatchToolParams, dispatch_tool};

/// 单次工具调用的 registry 分发面（不含 `workflow_execute`）。
#[async_trait]
pub trait ToolDispatch: Send + Sync {
    async fn dispatch(&self, p: DispatchToolParams<'_>) -> (String, Option<serde_json::Value>);
}

/// 默认实现：转发至 [`dispatch_tool`]。
#[derive(Debug, Clone, Copy, Default)]
pub struct InternalToolDispatch;

#[async_trait]
impl ToolDispatch for InternalToolDispatch {
    async fn dispatch(&self, p: DispatchToolParams<'_>) -> (String, Option<serde_json::Value>) {
        dispatch_tool(p).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::tool_registry::{
        DispatchToolCall, DispatchToolMemory, DispatchToolObs, DispatchToolParams,
        DispatchToolPolicy, DispatchToolWorkspace, HandlerLookupTable, ToolRuntime,
    };
    use crate::tool_sandbox::default_sync_default_sandbox_backend;
    use crabmate_config::load_config;
    use crabmate_types::{FunctionCall, ToolCall};

    struct CountingMockDispatch {
        hits: AtomicUsize,
        body: &'static str,
    }

    #[async_trait]
    impl ToolDispatch for CountingMockDispatch {
        async fn dispatch(
            &self,
            _p: DispatchToolParams<'_>,
        ) -> (String, Option<serde_json::Value>) {
            self.hits.fetch_add(1, Ordering::SeqCst);
            (self.body.to_string(), None)
        }
    }

    #[tokio::test]
    async fn mock_tool_dispatch_returns_fixed_body_without_registry() {
        let cfg = Arc::new(load_config(None).expect("embed default"));
        let wd = Path::new(".");
        let mut workspace_changed = false;
        let runtime = ToolRuntime {
            workspace_changed: &mut workspace_changed,
            ctx: None,
        };
        let tc = ToolCall {
            id: "call_mock".into(),
            typ: "function".into(),
            function: FunctionCall {
                name: "get_current_time".into(),
                arguments: "{}".into(),
            },
        };
        let lookup = HandlerLookupTable::default_dispatch();
        let sandbox = default_sync_default_sandbox_backend();
        let mock = CountingMockDispatch {
            hits: AtomicUsize::new(0),
            body: "mock-tool-ok",
        };
        let (out, inject) = mock
            .dispatch(DispatchToolParams {
                runtime,
                call: DispatchToolCall {
                    name: "get_current_time",
                    args: "{}",
                    tc: &tc,
                },
                workspace: DispatchToolWorkspace {
                    effective_working_dir: wd,
                    workspace_is_set: true,
                    workspace_changelist: None,
                },
                policy: DispatchToolPolicy {
                    cfg: &cfg,
                    turn_allow: None,
                    handler_lookup: &lookup,
                    sync_default_sandbox_backend: &sandbox,
                },
                obs: DispatchToolObs {
                    sse_out_tx: None,
                    sse_control_mirror: None,
                },
                memory: DispatchToolMemory {
                    read_file_turn_cache: None,
                    long_term_memory: None,
                    long_term_memory_scope_id: None,
                    mcp_turn: None,
                },
            })
            .await;
        assert_eq!(out, "mock-tool-ok");
        assert!(inject.is_none());
        assert_eq!(mock.hits.load(Ordering::SeqCst), 1);
    }
}
