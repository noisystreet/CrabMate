//! 进程级 Prompt 缓存累积统计（无锁原子操作）。

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use crabmate_types::Usage;

/// 进程级缓存累积统计。
#[derive(Debug, Default)]
pub struct LlmCacheAggregate {
    total_hit_tokens: AtomicU64,
    total_miss_tokens: AtomicU64,
    request_count: AtomicU64,
}

impl LlmCacheAggregate {
    /// 记录一次 LLM 调用的缓存使用情况。
    pub fn record(&self, usage: &Usage) {
        if let Some(hit) = usage.prompt_cache_hit_tokens {
            self.total_hit_tokens.fetch_add(hit, Ordering::Relaxed);
        }
        if let Some(miss) = usage.prompt_cache_miss_tokens {
            self.total_miss_tokens.fetch_add(miss, Ordering::Relaxed);
        }
        self.request_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 缓存命中率（0.0–1.0）。
    pub fn hit_ratio(&self) -> f64 {
        let hit = self.total_hit_tokens.load(Ordering::Relaxed);
        let miss = self.total_miss_tokens.load(Ordering::Relaxed);
        let total = hit + miss;
        if total == 0 {
            0.0
        } else {
            hit as f64 / total as f64
        }
    }

    /// 总请求数。
    pub fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }

    /// 总缓存命中 token 数。
    pub fn total_hit_tokens(&self) -> u64 {
        self.total_hit_tokens.load(Ordering::Relaxed)
    }

    /// 估算节省金额（USD）。
    ///
    /// - `miss_rate`: 未命中单价**每 token**（如 V4-Flash `$0.14/1M` → `0.14e-6`）
    /// - `hit_rate`: 命中单价**每 token**（如 `$0.014/1M` → `0.014e-6`）
    pub fn estimated_savings(&self, miss_rate: f64, hit_rate: f64) -> f64 {
        let hit = self.total_hit_tokens.load(Ordering::Relaxed);
        hit as f64 * (miss_rate - hit_rate)
    }
}

/// 全局单例（进程级）。
pub static LLM_CACHE_AGGREGATE: LazyLock<LlmCacheAggregate> =
    LazyLock::new(LlmCacheAggregate::default);

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_usage(hit: u64, miss: u64) -> Usage {
        Usage {
            input_tokens: Some(hit + miss),
            output_tokens: Some(0),
            prompt_cache_hit_tokens: Some(hit),
            prompt_cache_miss_tokens: Some(miss),
        }
    }

    #[test]
    fn record_and_ratio() {
        let agg = LlmCacheAggregate::default();
        agg.record(&sample_usage(100, 900));
        assert!((agg.hit_ratio() - 0.1).abs() < 1e-6);
        assert_eq!(agg.request_count(), 1);
        assert_eq!(agg.total_hit_tokens(), 100);
    }

    #[test]
    fn multiple_records_accumulate() {
        let agg = LlmCacheAggregate::default();
        agg.record(&sample_usage(100, 900));
        agg.record(&sample_usage(200, 300));
        assert_eq!(agg.request_count(), 2);
        assert_eq!(agg.total_hit_tokens(), 300);
        assert!((agg.hit_ratio() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn empty_no_panics() {
        let agg = LlmCacheAggregate::default();
        assert_eq!(agg.hit_ratio(), 0.0);
        assert_eq!(agg.estimated_savings(0.14e-6, 0.014e-6), 0.0);
    }

    #[test]
    fn recorded_savings_is_positive() {
        let agg = LlmCacheAggregate::default();
        agg.record(&sample_usage(1_000_000, 0));
        let s = agg.estimated_savings(0.14e-6, 0.014e-6);
        assert!((s - 0.126).abs() < 1e-4);
    }

    #[test]
    fn hit_ratio_zero_when_no_miss() {
        let agg = LlmCacheAggregate::default();
        agg.record(&sample_usage(0, 100));
        assert_eq!(agg.hit_ratio(), 0.0);
    }

    #[test]
    fn hit_ratio_one_when_no_miss() {
        let agg = LlmCacheAggregate::default();
        agg.record(&sample_usage(100, 0));
        assert_eq!(agg.hit_ratio(), 1.0);
    }
}
