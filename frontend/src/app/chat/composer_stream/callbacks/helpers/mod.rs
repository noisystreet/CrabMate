//! 时间线/气泡/子目标等辅助（供 [`super::builders`] 与 [`super::assemble`] 使用）。
//!
//! 按职责拆分为子模块，降低单文件圈复杂度；对外符号经本 `mod` 再导出，保持 `super::helpers::*` 路径稳定。

mod indices;
mod stream_diag;
mod text_format;
mod timeline_tail;

pub(crate) use indices::*;
pub(crate) use stream_diag::*;
pub(crate) use text_format::*;
pub(crate) use timeline_tail::*;
