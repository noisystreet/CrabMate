//! CrabMate 工具支撑层：工作区路径、工具结果类型、内置 Function Calling 工具实现等。

pub use crabmate_config;
pub use crabmate_config as config;
pub use crabmate_types;
pub use crabmate_types as types;

pub mod cargo_metadata;
pub mod clarification_questionnaire;
pub mod health_dep_compat;
pub mod memory_tool_host;
pub mod project_metrics;
pub mod project_profile;
pub mod read_file_turn_cache;
pub mod redact;
pub mod registry_policy;
pub mod text_encoding;
pub mod tool_dispatch;
pub mod tool_naming;
pub mod tool_result;
pub mod tool_runtime;
pub mod tools;
pub mod workspace;
