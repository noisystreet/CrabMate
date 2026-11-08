//! 工具宿主桶（S4-P1c 门面，见 #1300 §1.4）。
//!
//! 10 个成员已在 S4-P2-4 / P2-5 全部就位：`tool_registry`（P2-4）与其余 8 个（P2-5）。
//! `tool_sandbox` 按 §4.2 定案提到 **crate 顶层**（`crate::tool_sandbox`），**不进**本桶 ——
//! 它有公开面，塞进 `pub(crate)` 桶会制造假结构。
//!
//! `lib_server.rs` 的 `pub(crate) use` 指本桶：根上短名 `crate::tool_registry` 等不变，
//! 变的是定义点。

pub mod dynamic_tools;
pub mod long_term_memory_tools;
pub mod memory_tool_hosts;
pub mod readonly_tool_ttl_cache;
pub mod terminal_session;
pub mod tool_approval;
pub mod tool_call_explain;
pub mod tool_registry;
pub mod tool_stats;
