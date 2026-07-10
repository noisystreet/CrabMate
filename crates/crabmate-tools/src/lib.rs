//! CrabMate 工具支撑层：工作区路径、工具结果类型、文本编码、脱敏工具等。
//!
//! 从 `crabmate-internal` 拆分而来，为 `crabmate-internal` 和 `crabmate-memory` 提供基础类型与工具函数。

pub use crabmate_config;
pub use crabmate_config as config;
pub use crabmate_types;
pub use crabmate_types as types;

pub mod cargo_metadata;
pub mod redact;
pub mod text_encoding;
pub mod tool_result;
pub mod workspace;
