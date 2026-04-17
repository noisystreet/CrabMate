//! Manager Agent：任务分解与协调
//!
//! Manager 负责：
//! - 理解高层任务目标
//! - 分解为可执行的 SubGoals
//! - 确定执行策略
//! - 协调子目标执行

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{
    CompleteChatRetryingParams, LlmRetryingTransportOpts, complete_chat_retrying,
    no_tools_chat_request,
};
use crate::types::{LlmSeedOverride, Message, message_content_as_str};

use super::task::{Capability, ExecutionStrategy, SubGoal, TaskResult, TaskStatus};

/// Manager Agent 配置
#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// 最大子目标数量
    pub max_sub_goals: usize,
    /// 执行策略
    pub execution_strategy: ExecutionStrategy,
    /// 是否启用
    pub enabled: bool,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            max_sub_goals: 10,
            execution_strategy: ExecutionStrategy::Hybrid,
            enabled: true,
        }
    }
}

/// Manager Agent 错误
#[derive(Debug)]
pub enum ManagerError {
    LlmError(String),
    ParseError(String),
    ExecutionError(String),
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManagerError::LlmError(s) => write!(f, "LLM error: {}", s),
            ManagerError::ParseError(s) => write!(f, "Parse error: {}", s),
            ManagerError::ExecutionError(s) => write!(f, "Execution error: {}", s),
        }
    }
}

impl std::error::Error for ManagerError {}

/// Manager Agent
pub struct ManagerAgent {
    config: ManagerConfig,
}

impl ManagerAgent {
    pub fn new(config: ManagerConfig) -> Self {
        Self { config }
    }

    /// 分解任务为子目标（使用 LLM）
    pub async fn decompose_with_llm(
        &self,
        task: &str,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<ManagerOutput, ManagerError> {
        let prompt = self.build_decomposition_prompt(task);

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
                self.parse_output(&content)
            }
            Err(e) => {
                log::warn!(target: "crabmate", "Manager LLM call failed: {}, falling back to simple decomposition", e);
                Ok(self.decompose_fallback(task))
            }
        }
    }

    /// 降级分解（不调用 LLM）
    fn decompose_fallback(&self, task: &str) -> ManagerOutput {
        let sub_goals = vec![
            SubGoal::new("goal_1", task)
                .with_capabilities(vec![Capability::FileRead, Capability::CommandExecution]),
        ];

        ManagerOutput {
            sub_goals,
            execution_strategy: self.config.execution_strategy,
            summary: format!("Simple decomposition for: {}", task),
        }
    }

    /// 构建分解 prompt
    fn build_decomposition_prompt(&self, task: &str) -> String {
        format!(
            r#"## 任务
你是一个任务分解专家。请将以下用户任务分解为可执行的子目标。

任务：{}

## 要求
1. 每个子目标应该是独立的、可验证的
2. 考虑子目标之间的依赖关系
3. 识别可以并行执行的子目标
4. 为每个子目标分配合适的工具能力

## 可用能力
- FileRead: 文件读取、搜索
- FileWrite: 文件写入、创建
- CommandExecution: 命令执行
- NetworkRequest: HTTP 请求
- WebSearch: 网页搜索

## 输出格式
请严格按以下 JSON 格式输出，只输出 JSON，不要有其他内容：
{{
    "sub_goals": [
        {{
            "goal_id": "goal_1",
            "description": "子目标描述",
            "priority": 1,
            "depends_on": [],
            "required_capabilities": ["FileRead"]
        }}
    ],
    "execution_strategy": "hybrid"
}}

## 约束
- 子目标数量不超过 {}
- 只输出 JSON
"#,
            task, self.config.max_sub_goals
        )
    }

    /// 解析 LLM 输出
    fn parse_output(&self, content: &str) -> Result<ManagerOutput, ManagerError> {
        let json_str = extract_json(content).ok_or_else(|| {
            ManagerError::ParseError("Failed to extract JSON from response".to_string())
        })?;

        #[derive(serde::Deserialize)]
        struct OutputJson {
            sub_goals: Vec<SubGoalJson>,
            execution_strategy: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct SubGoalJson {
            goal_id: String,
            description: String,
            priority: Option<u32>,
            depends_on: Option<Vec<String>>,
            required_capabilities: Option<Vec<String>>,
        }

        let parsed: OutputJson =
            serde_json::from_str(json_str).map_err(|e| ManagerError::ParseError(e.to_string()))?;

        let execution_strategy = parsed
            .execution_strategy
            .as_ref()
            .map(|s| match s.as_str() {
                "sequential" => ExecutionStrategy::Sequential,
                "parallel" => ExecutionStrategy::Parallel,
                _ => ExecutionStrategy::Hybrid,
            })
            .unwrap_or(self.config.execution_strategy);

        let mut sub_goals = Vec::new();
        for sg in parsed.sub_goals {
            let capabilities = sg
                .required_capabilities
                .unwrap_or_default()
                .into_iter()
                .filter_map(|c| match c.as_str() {
                    "FileRead" => Some(Capability::FileRead),
                    "FileWrite" => Some(Capability::FileWrite),
                    "CommandExecution" => Some(Capability::CommandExecution),
                    "NetworkRequest" => Some(Capability::NetworkRequest),
                    "WebSearch" => Some(Capability::WebSearch),
                    _ => None,
                })
                .collect();

            sub_goals.push(SubGoal {
                goal_id: sg.goal_id,
                description: sg.description,
                priority: sg.priority.unwrap_or(0),
                depends_on: sg.depends_on.unwrap_or_default(),
                required_capabilities: capabilities,
            });
        }

        let summary = format!("Decomposed into {} sub-goals", sub_goals.len());
        Ok(ManagerOutput {
            sub_goals,
            execution_strategy,
            summary,
        })
    }

    /// 获取执行策略
    pub fn execution_strategy(&self) -> ExecutionStrategy {
        self.config.execution_strategy
    }
}

/// Manager 输出
#[derive(Debug, Clone)]
pub struct ManagerOutput {
    /// 分解的子目标列表
    pub sub_goals: Vec<SubGoal>,
    /// 执行策略
    pub execution_strategy: ExecutionStrategy,
    /// 给用户的结果摘要
    pub summary: String,
}

/// 失败处理决策
#[derive(Debug, Clone)]
pub enum FailureDecision {
    Continue,
    Retry { goal_id: String },
    Skip { goal_id: String, reason: String },
    Replan { reason: String },
    Abort { reason: String },
}

/// 处理失败
pub fn handle_failure(
    results: &[TaskResult],
    max_failures: usize,
) -> (Vec<&TaskResult>, Vec<&TaskResult>, FailureDecision) {
    let completed: Vec<&TaskResult> = results
        .iter()
        .filter(|r| matches!(r.status, TaskStatus::Completed))
        .collect();

    let failed: Vec<&TaskResult> = results
        .iter()
        .filter(|r| matches!(r.status, TaskStatus::Failed { .. }))
        .collect();

    let decision = if failed.is_empty() {
        FailureDecision::Continue
    } else if failed.len() > max_failures {
        FailureDecision::Abort {
            reason: format!("{} failures exceeded threshold", failed.len()),
        }
    } else {
        FailureDecision::Continue
    };

    (completed, failed, decision)
}

/// 从响应中提取 JSON
fn extract_json(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}').map(|i| i + 1)?;
    let json_str = &content[start..end];
    if json_str.starts_with('{') && json_str.ends_with('}') {
        Some(json_str)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_decompose_fallback() {
        let manager = ManagerAgent::new(ManagerConfig::default());
        let output = manager.decompose_fallback("测试任务");
        assert_eq!(output.sub_goals.len(), 1);
    }

    #[test]
    fn test_extract_json() {
        let content = "好的，我来分解。\n{\n  \"sub_goals\": []\n}\n完成";
        let json = extract_json(content).unwrap();
        assert!(json.contains("sub_goals"));
    }
}
