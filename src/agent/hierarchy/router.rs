//! 路由层：根据任务复杂度选择执行模式

use super::task::ExecutionStrategy;

/// Agent 执行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentMode {
    /// 单一 Agent（现有默认模式）
    #[default]
    Single,
    /// 分层架构（Manager + Operator）
    Hierarchical,
    /// 多 Agent 群体
    MultiAgent,
    /// 纯 ReAct
    ReAct,
}

/// 任务复杂度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskComplexity {
    Simple,      // 1-2 步
    Medium,      // 3-5 步
    Complex,     // 6-20 步
    VeryComplex, // 20+ 步
}

/// 路由决策结果
#[derive(Debug, Clone)]
pub struct RouterOutput {
    pub mode: AgentMode,
    pub max_iterations: usize,
    pub max_sub_goals: usize,
    pub execution_strategy: ExecutionStrategy,
}

impl Default for RouterOutput {
    fn default() -> Self {
        Self {
            mode: AgentMode::Single,
            max_iterations: 10,
            max_sub_goals: 10,
            execution_strategy: ExecutionStrategy::Hybrid,
        }
    }
}

/// 路由器
pub struct Router;

impl Router {
    /// 根据任务内容进行路由决策
    pub fn route(task: &str) -> RouterOutput {
        let complexity = Self::estimate_complexity(task);

        match complexity {
            TaskComplexity::Simple => RouterOutput {
                mode: AgentMode::Single,
                max_iterations: 5,
                max_sub_goals: 3,
                execution_strategy: ExecutionStrategy::Sequential,
            },
            TaskComplexity::Medium => RouterOutput {
                mode: AgentMode::ReAct,
                max_iterations: 10,
                max_sub_goals: 5,
                execution_strategy: ExecutionStrategy::Hybrid,
            },
            TaskComplexity::Complex => RouterOutput {
                mode: AgentMode::Hierarchical,
                max_iterations: 30,
                max_sub_goals: 20,
                execution_strategy: ExecutionStrategy::Hybrid,
            },
            TaskComplexity::VeryComplex => RouterOutput {
                mode: AgentMode::MultiAgent,
                max_iterations: 50,
                max_sub_goals: 50,
                execution_strategy: ExecutionStrategy::Parallel,
            },
        }
    }

    /// 估算任务复杂度
    fn estimate_complexity(task: &str) -> TaskComplexity {
        let task_lower = task.to_lowercase();
        let mut score = 0usize;

        // 关键词评估
        let analysis_keywords = ["分析", "比较", "评估", "调研"];
        for kw in analysis_keywords {
            if task_lower.contains(kw) {
                score += 2;
            }
        }

        let parallel_keywords = ["多个", "并行", "同时", "分别"];
        for kw in parallel_keywords {
            if task_lower.contains(kw) {
                score += 3;
            }
        }

        let complex_keywords = ["测试", "修改", "重构", "迁移", "部署"];
        for kw in complex_keywords {
            if task_lower.contains(kw) {
                score += 2;
            }
        }

        // 工具需求预估
        let tool_keywords = [
            "文件",
            "代码",
            "测试",
            "编译",
            "部署",
            "API",
            "数据库",
            "配置",
        ];
        for kw in tool_keywords {
            if task_lower.contains(kw) {
                score += 1;
            }
        }

        // 步骤数量预估
        let step_indicators = ["1.", "2.", "3.", "首先", "然后", "接着", "最后"];
        for indicator in step_indicators {
            if task_lower.contains(indicator) {
                score += 1;
            }
        }

        match score {
            0..=2 => TaskComplexity::Simple,
            3..=5 => TaskComplexity::Medium,
            6..=10 => TaskComplexity::Complex,
            _ => TaskComplexity::VeryComplex,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_task() {
        let output = Router::route("帮我读取 /tmp/test.txt 文件内容");
        assert_eq!(output.mode, AgentMode::Single);
    }

    #[test]
    fn test_complex_task() {
        let output = Router::route("读取代码并分析测试覆盖率");
        assert_eq!(output.mode, AgentMode::Hierarchical);
    }
}
