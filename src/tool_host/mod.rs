//! 工具宿主桶（S4-P1c 门面，见 #1300 §1.4）。
//!
//! **本文件当前只做转发**：成员仍住在 `cm_internal`，P2 才逐个搬进来（`tool_registry`
//! 23 文件 / 5473 行独占一期）。建门面的意义是给根上短名一个稳定落点 —— `lib_server.rs`
//! 的 `pub(crate) use` 已改指本桶，P2 搬文件时不会牵动任何调用点。
//!
//! 成员 10 子项：`tool_registry`、`terminal_session`、`tool_sandbox`、`memory_tool_hosts`、
//! `tool_stats`、`dynamic_tools`、`tool_approval`、`long_term_memory_tools`、
//! `tool_call_explain`、`readonly_tool_ttl_cache`。

// 这些成员有根上短名消费者，由 `lib_server.rs` 的 re-export 使用。
pub(crate) use crate::cm_internal::{
    memory_tool_hosts, readonly_tool_ttl_cache, tool_call_explain, tool_registry, tool_stats,
};

// 这几个成员**没有短名消费者**，但性质分两类，不要一视同仁：
//   · `dynamic_tools`（4 处长路径）与 `tool_sandbox`（1 处）是**在用**的 —— 上层直接写
//     `crate::cm_internal::<x>` 绕过了桶，P3 接线时改指本桶即可；
//   · `long_term_memory_tools` / `terminal_session` / `tool_approval` 当前 **0 引用**，
//     属死代码候选。P2 搬进来时要单独判断是接线还是删除 —— **别因为进了桶就当它是活的**。
// P2 搬入后桶内直接可见，届时删掉这条转发即可。用 **item 级** allow —— 模块级罩子会把
// 真实死代码一起遮住（#1229 的教训）。
#[allow(unused_imports)]
pub(crate) use crate::cm_internal::{
    dynamic_tools, long_term_memory_tools, terminal_session, tool_approval, tool_sandbox,
};
