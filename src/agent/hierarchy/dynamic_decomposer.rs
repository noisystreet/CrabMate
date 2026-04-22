//! 动态子目标分解器
//!
//! 当 Operator 检测到当前目标过于复杂或执行困难时，
//! 动态调用 LLM 进行子目标分解，实现递归分解能力。

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{
    CompleteChatRetryingParams, LlmRetryingTransportOpts, complete_chat_retrying,
    no_tools_chat_request,
};
use crate::types::{LlmSeedOverride, Message, message_content_as_str};

use super::task::{SubGoal, TaskResult};

/// 复杂度评估结果
#[derive(Debug, Clone)]
pub struct ComplexityAssessment {
    /// 复杂度评分 (0-100)
    pub score: u8,
    /// 是否需要分解
    pub needs_decomposition: bool,
    /// 原因说明
    pub reason: String,
    /// 建议的子目标数量
    pub suggested_subgoals: usize,
}

/// 动态分解器
pub struct DynamicDecomposer;

impl DynamicDecomposer {
    /// 创建新的动态分解器
    pub fn new() -> Self {
        Self
    }

    /// 评估目标复杂度
    ///
    /// 基于以下指标评估：
    /// - 目标描述长度和复杂度
    /// - 历史执行失败次数
    /// - 已用迭代次数
    /// - 涉及的工具数量
    pub fn assess_complexity(
        &self,
        goal: &SubGoal,
        iterations: usize,
        consecutive_failures: usize,
        tools_used: usize,
    ) -> ComplexityAssessment {
        let mut score = 0u8;
        let mut reasons = Vec::new();

        // 基于迭代次数评分
        if iterations > 10 {
            score += 25;
            reasons.push(format!("已执行 {} 轮迭代", iterations));
        } else if iterations > 5 {
            score += 15;
            reasons.push(format!("已执行 {} 轮迭代", iterations));
        }

        // 基于连续失败次数评分
        if consecutive_failures >= 3 {
            score += 30;
            reasons.push(format!("连续 {} 次失败", consecutive_failures));
        } else if consecutive_failures >= 2 {
            score += 15;
            reasons.push(format!("连续 {} 次失败", consecutive_failures));
        }

        // 基于目标描述复杂度评分
        let desc_len = goal.description.len();
        if desc_len > 500 {
            score += 20;
            reasons.push("目标描述较复杂".to_string());
        } else if desc_len > 200 {
            score += 10;
        }

        // 基于使用工具数量评分
        if tools_used > 5 {
            score += 15;
            reasons.push("涉及多个工具".to_string());
        }

        // 检查是否包含复杂关键词
        let complex_keywords = [
            "重构",
            "重构",
            "架构",
            "设计",
            "优化",
            "性能",
            "refactor",
            "architecture",
            "design",
            "optimize",
            "performance",
            "多步骤",
            "multi-step",
            "复杂",
            "complex",
        ];
        for keyword in &complex_keywords {
            if goal.description.to_lowercase().contains(keyword) {
                score += 10;
                reasons.push(format!("包含复杂关键词: {}", keyword));
                break;
            }
        }

        // 建议的子目标数量
        let suggested_subgoals = if score > 70 {
            5
        } else if score > 50 {
            4
        } else if score > 30 {
            3
        } else {
            2
        };

        ComplexityAssessment {
            score: score.min(100),
            needs_decomposition: score >= 30,
            reason: if reasons.is_empty() {
                "复杂度正常".to_string()
            } else {
                reasons.join("; ")
            },
            suggested_subgoals,
        }
    }

    /// 动态分解目标为子目标
    ///
    /// 当检测到目标过于复杂时，调用 LLM 进行动态分解
    #[allow(clippy::too_many_arguments)]
    pub async fn decompose(
        &self,
        parent_goal: &SubGoal,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
        execution_history: &[TaskResult],
        current_iteration: usize,
    ) -> Result<Vec<SubGoal>, DynamicDecomposeError> {
        log::info!(
            target: "crabmate",
            "[DYNAMIC_DECOMPOSER] Decomposing goal_id={} at iteration={}",
            parent_goal.goal_id, current_iteration
        );

        let prompt =
            self.build_decomposition_prompt(parent_goal, execution_history, current_iteration);

        let messages = vec![Message::user_only(&prompt)];
        let request =
            no_tools_chat_request(cfg, &messages, None, None, LlmSeedOverride::FromConfig);

        let params = CompleteChatRetryingParams::new(
            llm_backend,
            client,
            api_key,
            cfg,
            LlmRetryingTransportOpts::headless_no_stream(),
            None,
            None,
        );

        match complete_chat_retrying(&params, &request).await {
            Ok((response, _)) => {
                let content = message_content_as_str(&response.content)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                self.parse_decomposition(&content, parent_goal)
            }
            Err(e) => {
                log::error!(
                    target: "crabmate",
                    "[DYNAMIC_DECOMPOSER] LLM call failed: {}", e
                );
                Err(DynamicDecomposeError::LlmError(e.to_string()))
            }
        }
    }

    /// 构建分解提示词
    fn build_decomposition_prompt(
        &self,
        goal: &SubGoal,
        execution_history: &[TaskResult],
        current_iteration: usize,
    ) -> String {
        let history_summary = if execution_history.is_empty() {
            "暂无执行历史".to_string()
        } else {
            execution_history
                .iter()
                .map(|r| format!("- {}: {:?}", r.task_id, r.status))
                .collect::<Vec<_>>()
                .join("\n")
        };

        format!(
            r#"你是一个任务分解专家。当前子目标执行遇到困难，需要将其分解为更小、更易执行的子目标。

## 父目标
目标ID: {}
描述: {}

## 执行历史
已执行迭代: {}
历史记录:
{}

## 任务
请将上述父目标分解为 2-4 个更小、更具体的子目标。每个子目标应该：
1. 独立可执行（有明确的完成标准）
2. 比父目标更简单（步骤更少、范围更小）
3. 按依赖关系排序（前面的子目标不依赖后面的）
4. 使用清晰、可操作的描述

## 输出格式
请严格按照以下 JSON 格式输出（不要包含 markdown 代码块标记）：

{{
  "sub_goals": [
    {{
      "goal_id": "sub_goal_1",
      "description": "第一个子目标的具体描述",
      "rationale": "为什么需要这个子目标"
    }},
    {{
      "goal_id": "sub_goal_2",
      "description": "第二个子目标的具体描述",
      "rationale": "为什么需要这个子目标"
    }}
  ],
  "reasoning": "分解的整体思路和依赖关系说明"
}}

请确保输出是有效的 JSON 格式。"#,
            goal.goal_id, goal.description, current_iteration, history_summary
        )
    }

    /// 解析分解结果
    fn parse_decomposition(
        &self,
        content: &str,
        parent_goal: &SubGoal,
    ) -> Result<Vec<SubGoal>, DynamicDecomposeError> {
        // 尝试提取 JSON 部分
        let json_str = if content.contains("```json") {
            content
                .split("```json")
                .nth(1)
                .and_then(|s| s.split("```").next())
                .unwrap_or(content)
                .trim()
        } else if content.contains("```") {
            content.split("```").nth(1).unwrap_or(content).trim()
        } else {
            content.trim()
        };

        #[derive(serde::Deserialize)]
        struct SubGoalDef {
            goal_id: String,
            description: String,
            #[allow(dead_code)]
            rationale: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct DecompositionOutput {
            sub_goals: Vec<SubGoalDef>,
            #[allow(dead_code)]
            reasoning: Option<String>,
        }

        match serde_json::from_str::<DecompositionOutput>(json_str) {
            Ok(output) => {
                let sub_goals: Vec<SubGoal> = output
                    .sub_goals
                    .into_iter()
                    .map(|sg| {
                        let mut sub_goal = SubGoal::new(
                            &format!("{}_{}", parent_goal.goal_id, sg.goal_id),
                            &sg.description,
                        );
                        // 继承父目标的验收条件
                        sub_goal.acceptance = parent_goal.acceptance.clone();
                        sub_goal
                    })
                    .collect();

                log::info!(
                    target: "crabmate",
                    "[DYNAMIC_DECOMPOSER] Successfully decomposed into {} sub-goals",
                    sub_goals.len()
                );

                Ok(sub_goals)
            }
            Err(e) => {
                log::error!(
                    target: "crabmate",
                    "[DYNAMIC_DECOMPOSER] Failed to parse decomposition: {}", e
                );
                Err(DynamicDecomposeError::ParseError(format!(
                    "JSON parse error: {}. Content: {}",
                    e, content
                )))
            }
        }
    }
}

impl Default for DynamicDecomposer {
    fn default() -> Self {
        Self::new()
    }
}

/// 动态分解错误
#[derive(Debug)]
pub enum DynamicDecomposeError {
    LlmError(String),
    ParseError(String),
}

impl std::fmt::Display for DynamicDecomposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DynamicDecomposeError::LlmError(s) => write!(f, "LLM error: {}", s),
            DynamicDecomposeError::ParseError(s) => write!(f, "Parse error: {}", s),
        }
    }
}

impl std::error::Error for DynamicDecomposeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complexity_assessment() {
        let decomposer = DynamicDecomposer::new();

        let simple_goal = SubGoal::new("test_1", "简单任务");
        let assessment = decomposer.assess_complexity(&simple_goal, 2, 0, 2);
        assert!(!assessment.needs_decomposition);
        assert!(assessment.score < 30);

        let complex_goal = SubGoal::new(
            "test_2",
            "重构整个项目的架构，优化性能，这是一个非常复杂的多步骤任务",
        );
        let assessment = decomposer.assess_complexity(&complex_goal, 12, 3, 8);
        assert!(assessment.needs_decomposition);
        assert!(assessment.score >= 30);
    }

    #[test]
    fn test_parse_decomposition() {
        let decomposer = DynamicDecomposer::new();
        let parent = SubGoal::new("parent", "父目标");

        let json_content = r#"{
            "sub_goals": [
                {
                    "goal_id": "step1",
                    "description": "第一步",
                    "rationale": "需要先做这个"
                },
                {
                    "goal_id": "step2",
                    "description": "第二步",
                    "rationale": "然后做这个"
                }
            ],
            "reasoning": "分解思路"
        }"#;

        let result = decomposer.parse_decomposition(json_content, &parent);
        assert!(result.is_ok());
        let sub_goals = result.unwrap();
        assert_eq!(sub_goals.len(), 2);
        assert!(sub_goals[0].goal_id.starts_with("parent_"));
    }
}
