//! Benchmark 测评子系统：批量无人值守执行、适配器框架、指标采集与产物提取。
//!
//! 支持在主流 Agent benchmark（SWE-bench、GAIA、HumanEval 等）上对 CrabMate 进行能力测评。

pub mod adapter;
pub mod artifact;
pub mod metrics;
pub mod runner;
pub mod types;
