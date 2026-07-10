//! 并行墙钟、只读判定等与 `[tool_registry]` 配置对应的策略（实现见 `crabmate-tools`）。

pub use crabmate_tools::registry_policy::{
    http_fetch_outer_wall_secs, http_request_outer_wall_secs, is_readonly_tool,
    parallel_tool_wall_timeout_secs, sync_default_runs_inline,
    tool_calls_allow_parallel_sync_batch, tool_ok_for_parallel_readonly_batch_piece,
};
