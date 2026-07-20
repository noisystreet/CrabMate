//! Benchmark 测评子系统（核心类型与适配器已提取到 `crabmate-benchmark`）。
//!
//! 批量运行入口 `run_batch` 因依赖根包 `run_agent_turn`，保留在 `runner` 模块中。

// `types` 被外部模块引用；adapter/artifact/metrics 通过 `crabmate_benchmark` 直接使用。
pub use crabmate_benchmark::types;

pub mod runner;
