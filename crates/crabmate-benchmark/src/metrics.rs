//! Per-task 指标采集与批量汇总。

use serde::{Deserialize, Serialize};

use crate::types::{BenchmarkResult, TaskStatus};

/// 单条任务的运行指标。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskMetrics {
    pub wall_time_secs: f64,
    pub tool_calls_count: usize,
    pub agent_rounds: usize,
}

/// 批量运行后的汇总统计。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchSummary {
    pub total_tasks: usize,
    pub success_count: usize,
    pub timeout_count: usize,
    pub error_count: usize,
    pub max_rounds_count: usize,
    pub total_wall_time_secs: f64,
    pub avg_wall_time_secs: f64,
    pub total_tool_calls: usize,
}

impl BatchSummary {
    pub fn from_results(results: &[BenchmarkResult]) -> Self {
        let total = results.len();
        let mut s = Self {
            total_tasks: total,
            ..Default::default()
        };
        for r in results {
            match r.status {
                TaskStatus::Success => s.success_count += 1,
                TaskStatus::Timeout => s.timeout_count += 1,
                TaskStatus::Error => s.error_count += 1,
                TaskStatus::MaxRounds => s.max_rounds_count += 1,
            }
            s.total_wall_time_secs += r.metrics.wall_time_secs;
            s.total_tool_calls += r.metrics.tool_calls_count;
        }
        if total > 0 {
            s.avg_wall_time_secs = s.total_wall_time_secs / total as f64;
        }
        s
    }
}
