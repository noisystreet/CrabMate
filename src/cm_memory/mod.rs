//! CrabMate 长期记忆与语义索引。
//!
//! 从 `cm_internal` 拆分而来。

pub(crate) use crate::cm_config;
pub(crate) use crate::cm_config as config;
pub(crate) use crate::cm_types;
pub(crate) use crate::cm_types as types;

pub mod memory;
pub(crate) mod tool_check;
