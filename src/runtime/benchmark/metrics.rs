//! Per-task 指标采集与批量汇总。

use serde::{Deserialize, Serialize};

/// 单条任务的运行指标。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskMetrics {
    /// 任务实际耗时（秒，含工具执行）
    pub wall_time_secs: f64,
    /// Agent 循环中的工具调用总次数
    pub tool_calls_count: usize,
    /// Agent 循环的轮次数（一次 LLM 请求 + 可能的工具执行算一轮）
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
    pub fn from_results(results: &[super::types::BenchmarkResult]) -> Self {
        let total = results.len();
        let mut s = Self {
            total_tasks: total,
            ..Default::default()
        };
        for r in results {
            match r.status {
                super::types::TaskStatus::Success => s.success_count += 1,
                super::types::TaskStatus::Timeout => s.timeout_count += 1,
                super::types::TaskStatus::Error => s.error_count += 1,
                super::types::TaskStatus::MaxRounds => s.max_rounds_count += 1,
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
