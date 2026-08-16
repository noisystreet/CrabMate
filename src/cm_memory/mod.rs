//! CrabMate 长期记忆与语义索引。
//!
//! 从 `crabmate-internal` 拆分而来。

pub use crate::cm_config;
pub use crate::cm_config as config;
pub use crate::cm_types;
pub use crate::cm_types as types;

pub mod memory;
pub(crate) mod tool_check;
