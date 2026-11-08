//! 上下文组装桶（S4-P1c 建，S4-P2-2 起成员已迁入，见 #1300 §1.4）。
//!
//! 成员 7 子项：`context_bootstrap`、`agent_role_turn`、`agent_turn_prep`、`session_mode_turn`、
//! `agent_errors`、`clarification_sse_bridge`、`user_message_file_refs`。
//!
//! `user_message_file_refs` 归本桶而非 `tool_host`：它做的是「消息 → 上下文」解析，
//! 不是工具执行侧（§1.4 修正点 4，`data` 桶因此取消）。

pub mod agent_errors;
pub mod agent_role_turn;
pub mod agent_turn_prep;
pub mod clarification_sse_bridge;
pub mod context_bootstrap;
pub mod session_mode_turn;
pub mod user_message_file_refs;

// `lib_server.rs` 的根上 re-export 需要这个名字。P1c 时它是经 `cm_internal` 的**间接**
// 转发（`cm_internal::clarification_sse_bridge` 自己再 `pub use` 一次），搬迁后必须改为
// 桶内直连 —— 否则 `crate::cm_internal::clarification_questionnaire_body_if_tool_ok`
// 会变成一个看不出原因的 unresolved import。
pub(crate) use clarification_sse_bridge::clarification_questionnaire_body_if_tool_ok;
