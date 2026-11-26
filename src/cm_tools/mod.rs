//! CrabMate 工具支撑层：工作区路径、工具结果类型、内置 Function Calling 工具实现等。

pub mod cargo_metadata;
pub mod clarification_questionnaire;
/// #839 P1 契约层：`cm_tools` 侧只放类型与 trait，实现在 `crate::agent::delegation`。
///
/// **`allow(dead_code)` 是暂时的**：这一步刻意不接线（设计 §12「每步独立可验证」），
/// 因而 crate 内暂无消费者。消费者（`agent/delegation.rs` 与 `AGENT_LAYER_HANDLERS` 注册）
/// 落地后**必须先删掉这行**，否则等于把一个将来真死了的契约静默留着。
#[allow(dead_code)]
pub mod delegation_host;
pub mod fs_atomic;
pub mod github_token;
pub mod health_dep_compat;
pub mod memory_tool_host;
pub mod project_metrics;
pub mod project_profile;
pub mod read_file_turn_cache;
pub mod redact;
pub mod registry_policy;
pub mod text_encoding;
pub mod tool_dispatch;
pub mod tool_retry_policy;
pub mod tool_runtime;
pub mod tools;
pub mod workspace;
