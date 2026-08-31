//! CrabMate 工具支撑层：工作区路径、工具结果类型、内置 Function Calling 工具实现等。

pub(crate) use crate::cm_config;
pub(crate) use crate::cm_config as config;
pub(crate) use crate::cm_types;
pub(crate) use crate::cm_types as types;

pub mod cargo_metadata;
pub mod clarification_questionnaire;
pub mod github_token;
pub mod health_dep_compat;
pub mod memory_tool_host;
pub mod project_metrics;
pub mod project_profile;
pub mod read_file_turn_cache;
pub mod redact;
pub mod registry_policy;
pub mod subprocess_session;
pub mod text_encoding;
pub mod tool_dispatch;
pub mod tool_naming;
pub mod tool_result;
pub mod tool_retry_policy;
pub mod tool_runtime;
pub mod tools;
pub mod workspace;
