//! 工具名 → 执行类别 / 元数据 / 内部分发 id（实现见 `crabmate-tools`）。

pub use crabmate_tools::tool_dispatch::{
    HandlerId, HandlerLookupTable, ToolDispatchMeta, ToolExecutionClass, all_dispatch_metadata,
    execution_class_for_tool, try_dispatch_meta,
};
