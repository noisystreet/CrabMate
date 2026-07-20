//! CrabMate 测评子系统：批量无人值守执行、适配器框架、指标采集与产物提取。
//!
//! 支持在主流 Agent benchmark（SWE-bench、GAIA、HumanEval 等）上对 CrabMate 进行能力测评。
//! 批量执行入口 `run_batch` 位于根包 `crate::runtime::benchmark::runner`。

pub mod adapter;
pub mod artifact;
pub mod metrics;
pub mod types;
