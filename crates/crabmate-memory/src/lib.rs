//! CrabMate 长期记忆与语义索引。
//!
//! 从 `crabmate-internal` 拆分而来。

pub use crabmate_config;
pub use crabmate_config as config;
pub use crabmate_types;
pub use crabmate_types as types;

pub mod memory;
pub(crate) mod tool_check;
