//! 可观测性桶（S4-P1c 门面，见 #1300 §1.4）。
//!
//! **本文件当前只做转发**：成员仍住在 `cm_internal`，P2 才搬入。成员 3 子项：
//! `health`、`observability`、`request_chrome_trace`。
//!
//! `text_util` 曾在落位表里规划到 obs，§1.4 修正点 4 已将其**并入顶层**
//! （`crate::text_util`），不属本桶 —— 22 行的小工具凑进桶只会制造假结构。

pub(crate) use crate::cm_internal::{health, observability, request_chrome_trace};
