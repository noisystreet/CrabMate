//! 上下文组装桶（S4-P1c 门面，见 #1300 §1.4）。
//!
//! **本文件当前只做转发**：成员仍住在 `cm_internal`，P2 才搬入。成员 7 子项：
//! `context_bootstrap`、`agent_role_turn`、`agent_turn_prep`、`session_mode_turn`、
//! `agent_errors`、`clarification_sse_bridge`、`user_message_file_refs`。
//!
//! `user_message_file_refs` 归本桶而非 `tool_host`：它做的是「消息 → 上下文」解析，
//! 不是工具执行侧（§1.4 修正点 4，`data` 桶因此取消）。

// 有根上短名消费者的成员，由 `lib_server.rs` 的 re-export 使用。
pub(crate) use crate::cm_internal::{
    agent_errors, agent_role_turn, agent_turn_prep,
    clarification_questionnaire_body_if_tool_ok, context_bootstrap, session_mode_turn,
    user_message_file_refs,
};

// 暂无短名消费者：上层只需要上面那个函数，模块本身没人按名字引用。P2 搬入后桶内直接可见。
//
// ⚠️ P2 注意：上面那条 `clarification_questionnaire_body_if_tool_ok` 是**间接**转发 ——
// 它在 `cm_internal` 里本身又是从本模块转出的（`pub use clarification_sse_bridge::…`）。
// 一旦 `clarification_sse_bridge` 搬进来，`crate::cm_internal::clarification_questionnaire_body_if_tool_ok`
// 就不存在了，那条 use 必须改成 `pub(crate) use clarification_sse_bridge::clarification_questionnaire_body_if_tool_ok;`，
// 否则 P2 会撞上一个看不出原因的 unresolved import。
#[allow(unused_imports)]
pub(crate) use crate::cm_internal::clarification_sse_bridge;
