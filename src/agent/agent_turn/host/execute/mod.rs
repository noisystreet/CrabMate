//! **E（Execute）** 步：工具批执行。实现位于 **`tools`** 子模块；`agent_turn` 根再导出为 **`execute_tools`** 以保持既有路径。

pub(crate) mod tool_dispatch;
pub(crate) mod tool_execution_host;
pub(crate) mod tool_execution_trait;
pub(crate) mod tools;

#[allow(unused_imports)] // 稳定再导出；host 内亦可直接 `tool_dispatch::`
pub(crate) use tool_dispatch::{InternalToolDispatch, ToolDispatch};
pub(crate) use tool_execution_trait::{ParallelPrefetchParams, ToolExecutionHost};
