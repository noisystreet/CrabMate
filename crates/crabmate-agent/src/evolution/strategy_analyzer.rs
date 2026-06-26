//! 策略分析器：分析决策历史，生成行为策略建议。

use super::history_logger::DecisionRecord;

/// 策略建议结构
#[derive(Debug, Clone)]
pub struct StrategyHint {
    /// 策略维度
    pub dimension: &'static str,
    /// 建议内容
    pub suggestion: String,
    /// 置信度 0.0-1.0
    pub confidence: f32,
}

impl StrategyHint {
    /// 转为可注入 system prompt 的文本
    pub fn to_prompt_text(&self) -> String {
        format!("[策略建议 - {}] {}", self.dimension, self.suggestion)
    }
}

/// 策略分析器（无状态，接收记录集合进行分析）
pub struct StrategyAnalyzer {
    /// 最小样本数才触发建议
    min_samples: usize,
}

impl StrategyAnalyzer {
    pub fn new(min_samples: usize) -> Self {
        Self { min_samples }
    }

    /// 分析历史记录，生成策略建议
    pub fn analyze(&self, records: &[DecisionRecord]) -> Vec<StrategyHint> {
        let mut hints = Vec::new();

        if records.len() < self.min_samples {
            return hints;
        }

        // 1. 工具成功率分析
        hints.extend(self.analyze_tool_success_rate(records));

        // 2. 反思重写模式分析
        hints.extend(self.analyze_rewrite_pattern(records));

        // 3. 执行耗时分析
        hints.extend(self.analyze_execution_time(records));

        // 4. 工具使用频率分析
        hints.extend(self.analyze_tool_frequency(records));

        hints
    }

    /// 分析工具成功率
    fn analyze_tool_success_rate(&self, records: &[DecisionRecord]) -> Vec<StrategyHint> {
        let mut hints = Vec::new();

        // 按工具分组统计
        let mut tool_stats: std::collections::HashMap<&str, (usize, usize)> =
            std::collections::HashMap::new();

        for record in records {
            let entry = tool_stats
                .entry(record.tool_name.as_str())
                .or_insert((0, 0));
            entry.0 += 1;
            if record.success {
                entry.1 += 1;
            }
        }

        // 检测低成功率工具
        for (tool, &(total, successes)) in &tool_stats {
            if total >= 2 {
                let rate = successes as f32 / total as f32;
                if rate < 0.5 {
                    hints.push(StrategyHint {
                        dimension: "tool_selection",
                        suggestion: format!(
                            "检测到「{}」成功率仅 {:.0}%，建议优先使用更可靠的工具或调整参数策略。",
                            tool,
                            rate * 100.0
                        ),
                        confidence: (1.0 - rate).min(0.9),
                    });
                }
            }
        }

        hints
    }

    /// 分析反思重写模式
    fn analyze_rewrite_pattern(&self, records: &[DecisionRecord]) -> Vec<StrategyHint> {
        let mut hints = Vec::new();

        let avg_rewrite = if records.is_empty() {
            0.0
        } else {
            let total: usize = records.iter().map(|r| r.rewrite_count).sum();
            total as f32 / records.len() as f32
        };

        if avg_rewrite > 2.0 {
            hints.push(StrategyHint {
                dimension: "planning_efficiency",
                suggestion: format!(
                    "本会话平均反思重写次数为 {:.1}，建议先充分分析问题再行动，减少循环。",
                    avg_rewrite
                ),
                confidence: (avg_rewrite / 5.0).min(0.9),
            });
        }

        hints
    }

    /// 分析执行耗时
    fn analyze_execution_time(&self, records: &[DecisionRecord]) -> Vec<StrategyHint> {
        let mut hints = Vec::new();

        if records.is_empty() {
            return hints;
        }

        let total_ms: u64 = records.iter().map(|r| r.duration_ms).sum();
        let avg_ms = total_ms as f32 / records.len() as f32;

        // 慢工具检测
        for record in records {
            if record.duration_ms > 5000 && record.success {
                hints.push(StrategyHint {
                    dimension: "performance",
                    suggestion: format!(
                        "「{}」执行耗时 {}ms，建议检查参数或考虑并行化。",
                        record.tool_name, record.duration_ms
                    ),
                    confidence: 0.6,
                });
                break;
            }
        }

        // 整体效率提示
        // 整体效率提示
        if avg_ms > 2000.0 {
            hints.push(StrategyHint {
                dimension: "performance",
                suggestion: format!("当前工具执行平均耗时 {:.0}ms，注意上下文窗口限制。", avg_ms),
                confidence: 0.5,
            });
        }

        hints
    }

    /// 分析工具使用频率
    fn analyze_tool_frequency(&self, records: &[DecisionRecord]) -> Vec<StrategyHint> {
        let mut hints = Vec::new();

        let mut tool_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();

        for record in records {
            *tool_counts.entry(record.tool_name.as_str()).or_insert(0) += 1;
        }

        // 频繁使用的工具
        if let Some((&tool, &count)) = tool_counts.iter().max_by_key(|(_, c)| *c)
            && count > 3
        {
            hints.push(StrategyHint {
                dimension: "tool_selection",
                suggestion: format!(
                    "「{}」已使用 {} 次，考虑是否有更高效的替代方案（如语义搜索替代 grep）。",
                    tool, count
                ),
                confidence: 0.4,
            });
        }

        hints
    }
}

impl Default for StrategyAnalyzer {
    fn default() -> Self {
        Self::new(3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_low_success_rate_detection() {
        let analyzer = StrategyAnalyzer::new(2);

        let records = vec![
            DecisionRecord::from_tool_result("grep", "{}", false, 100, 0),
            DecisionRecord::from_tool_result("grep", "{}", false, 100, 0),
            DecisionRecord::from_tool_result("read_file", "{}", true, 50, 0),
        ];

        let hints = analyzer.analyze(&records);
        let tool_hints: Vec<_> = hints
            .iter()
            .filter(|h| h.dimension == "tool_selection")
            .collect();

        assert!(!tool_hints.is_empty());
        assert!(tool_hints[0].suggestion.contains("grep"));
    }

    #[test]
    fn test_rewrite_pattern() {
        let analyzer = StrategyAnalyzer::new(1);

        let records = vec![
            DecisionRecord::from_tool_result("search", "{}", true, 100, 3),
            DecisionRecord::from_tool_result("read", "{}", true, 50, 3),
        ];

        let hints = analyzer.analyze(&records);
        let rewrite_hints: Vec<_> = hints
            .iter()
            .filter(|h| h.dimension == "planning_efficiency")
            .collect();

        assert!(!rewrite_hints.is_empty());
    }
}
