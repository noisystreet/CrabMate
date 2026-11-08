//! 可观测性桶（S4-P1c 门面，见 #1300 §1.4）。
//!
//! **S4-P2-1 起成员已迁入本桶**：文件从 `cm_internal/` 直接移到本目录，模块声明由
//! `pub(crate) use crate::cm_internal::…` 改为本地 `pub mod`；上层 `crate::obs::health`
//! 与 `lib_server.rs` 的短名 re-export 路径不变。成员 3 子项：
//! `health`、`observability`、`request_chrome_trace`。
//!
//! `text_util` 曾在落位表里规划到 obs，§1.4 修正点 4 已将其**并入顶层**
//! （`crate::text_util`），不属本桶 —— 22 行的小工具凑进桶只会制造假结构。

pub mod health;
pub mod observability;
pub mod request_chrome_trace;
