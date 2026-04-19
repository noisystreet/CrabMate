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

use super::task::{ExecutionStrategy, SubGoal, TaskResult, TaskStatus};

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
#[derive(Clone)]
pub struct ManagerAgent {
    config: ManagerConfig,
}

impl ManagerAgent {
    pub fn new(config: ManagerConfig) -> Self {
        Self { config }
    }

    /// 分解任务为子目标（使用 LLM）
    #[allow(clippy::too_many_arguments)]
    pub async fn decompose_with_llm(
        &self,
        task: &str,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
    ) -> Result<ManagerOutput, ManagerError> {
        log::info!(target: "crabmate", "[HIERARCHICAL] Manager: decomposing task={}", truncate_task(task));

        let prompt = self.build_decomposition_prompt(task, working_dir, tools_defs);

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
        // 降级时创建一个通用目标，不指定具体工具
        let sub_goal = SubGoal::new("goal_1", task);
        ManagerOutput {
            sub_goals: vec![sub_goal],
            execution_strategy: self.config.execution_strategy,
            summary: format!("Simple decomposition for: {}", task),
        }
    }

    /// 基于执行结果和产物重新规划
    ///
    /// 当子目标执行失败或需要调整时，调用此方法让 Manager 重新分解任务，
    /// 结合已完成的 artifacts 和失败信息生成新的子目标计划。
    #[allow(clippy::too_many_arguments)]
    pub async fn replan_with_artifacts(
        &self,
        original_task: &str,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        previous_results: &[super::task::TaskResult],
        previous_artifacts: &[super::task::Artifact],
    ) -> Result<ManagerOutput, ManagerError> {
        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: replanning with {} previous results and {} artifacts",
            previous_results.len(),
            previous_artifacts.len()
        );

        let prompt = self.build_replan_prompt(
            original_task,
            working_dir,
            tools_defs,
            previous_results,
            previous_artifacts,
        );

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
                log::warn!(target: "crabmate", "Manager replan LLM call failed: {}, falling back to original plan", e);
                // 降级时返回原任务作为一个简单目标
                Ok(ManagerOutput {
                    sub_goals: vec![SubGoal::new("goal_1", original_task)],
                    execution_strategy: self.config.execution_strategy,
                    summary: "Replan failed, using simple decomposition".to_string(),
                })
            }
        }
    }

    /// 构建重新规划的 prompt
    fn build_replan_prompt(
        &self,
        original_task: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        previous_results: &[super::task::TaskResult],
        previous_artifacts: &[super::task::Artifact],
    ) -> String {
        let workspace_context = self.get_workspace_context(working_dir);
        let tools_description = self.format_tools_with_schemas(tools_defs);

        // 生成已完成 artifacts 的摘要
        let artifacts_summary = self.format_artifacts_summary(previous_artifacts);

        // 生成失败信息摘要
        let failures_summary = self.format_failures_summary(previous_results);

        format!(
            r#"## 任务
你是一个任务分解专家。原始任务需要重新规划。

原始任务：{}

## 工作目录上下文
{}
重要：子目标的描述应该基于实际存在的文件和目录。

## 已完成的产物（可供后续子目标使用）
{}
重要：后续子目标应该引用这些已创建的文件/产物，而不是重复创建。

## 失败信息
{}
重要：分析失败原因，在重新规划时避免相同的问题。

## 工具定义（完整参数 schema）
{}
重要：
- 分配工具时，确保工具参数能匹配子目标需求
- `run_command` 需要分别指定 `command`（命令名如 `ls`, `gcc`）和 `args`（参数数组），不要合并
- **`run_executable` 用于执行编译产物（如 cmake 构建后的可执行文件）**，不要用 `run_command` 执行 `./xxx`
- `read_dir` 默认只列出当前目录文件，路径必须是相对路径且不能包含 `..`
- `create_file` 的 `content` 参数必须是正确的 JSON 字符串（特殊字符需要转义）

## 输出格式
请严格按以下 JSON 格式输出，只输出 JSON，不要有其他内容：
{{
    "sub_goals": [
        {{
            "goal_id": "goal_1",
            "description": "子目标描述（基于实际文件结构和已有产物）",
            "priority": 1,
            "depends_on": ["goal_id_of_dependency"],
            "required_tools": ["tool_name1", "tool_name2"]
        }}
    ],
    "execution_strategy": "hybrid"
}}

## 约束
- 子目标数量不超过 {}
- 只输出 JSON
- 尽量复用已有的 artifacts，避免重复创建相同的文件
"#,
            original_task,
            workspace_context,
            artifacts_summary,
            failures_summary,
            tools_description,
            self.config.max_sub_goals
        )
    }

    /// 格式化 artifacts 摘要
    fn format_artifacts_summary(&self, artifacts: &[super::task::Artifact]) -> String {
        if artifacts.is_empty() {
            return "(尚无产物)".to_string();
        }

        let mut lines = Vec::new();
        for artifact in artifacts {
            let path_info = artifact
                .path
                .as_ref()
                .map(|p| format!(" (路径: {})", p))
                .unwrap_or_default();
            lines.push(format!(
                "- [{}] {}{}",
                format!("{:?}", artifact.kind).to_lowercase(),
                artifact.name,
                path_info
            ));
        }
        lines.join("\n")
    }

    /// 格式化失败摘要
    fn format_failures_summary(&self, results: &[super::task::TaskResult]) -> String {
        let failures: Vec<_> = results
            .iter()
            .filter(|r| matches!(r.status, super::task::TaskStatus::Failed { .. }))
            .collect();

        if failures.is_empty() {
            return "(无失败)".to_string();
        }

        let mut lines = Vec::new();
        for result in failures {
            let reason = match &result.status {
                super::task::TaskStatus::Failed { reason } => reason.clone(),
                _ => unreachable!(),
            };
            lines.push(format!("- 子目标 {} 失败: {}", result.task_id, reason));
        }
        lines.join("\n")
    }

    /// 构建分解 prompt
    fn build_decomposition_prompt(
        &self,
        task: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
    ) -> String {
        // 获取工作目录上下文
        let workspace_context = self.get_workspace_context(working_dir);

        // 生成完整工具定义（包含参数 schema）
        let tools_description = self.format_tools_with_schemas(tools_defs);

        format!(
            r#"## 任务
你是一个任务分解专家。请将以下用户任务分解为可执行的子目标。

任务：{}

## 工作目录上下文
{}
重要：子目标的描述应该基于实际存在的文件和目录。

## 工具定义（完整参数 schema）
{}
重要：
- 分配工具时，确保工具参数能匹配子目标需求
- `run_command` 需要分别指定 `command`（命令名如 `ls`, `gcc`）和 `args`（参数数组），不要合并
- **`run_executable` 用于执行编译产物（如 cmake 构建后的可执行文件）**，不要用 `run_command` 执行 `./xxx`
- `read_dir` 默认只列出当前目录文件，路径必须是相对路径且不能包含 `..`
- `create_file` 的 `content` 参数必须是正确的 JSON 字符串（特殊字符需要转义）

## 输出格式
请严格按以下 JSON 格式输出，只输出 JSON，不要有其他内容：
{{
    "sub_goals": [
        {{
            "goal_id": "goal_1",
            "description": "子目标描述（基于实际文件结构）",
            "priority": 1,
            "depends_on": [],
            "required_tools": ["tool_name1", "tool_name2"]
        }}
    ],
    "execution_strategy": "hybrid"
}}

## 约束
- 子目标数量不超过 {}
- 只输出 JSON
"#,
            task, workspace_context, tools_description, self.config.max_sub_goals
        )
    }

    /// 获取工作目录上下文信息
    fn get_workspace_context(&self, working_dir: &std::path::Path) -> String {
        let dir_path = working_dir.display();

        // 列出目录内容
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(working_dir) {
            for entry in read_dir.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    let path = entry.path();
                    let is_dir = path.is_dir();
                    let prefix = if is_dir { "[DIR] " } else { "[FILE]" };
                    entries.push(format!("{} {}", prefix, name));
                }
            }
        }

        let entries_str = if entries.is_empty() {
            "(目录为空或无法读取)".to_string()
        } else {
            entries.join("\n")
        };

        // 检查是否有 build 目录
        let build_info = if working_dir.join("build").is_dir() {
            "\n注意：存在 build/ 目录（CMake 构建产物可能在这里）".to_string()
        } else {
            String::new()
        };

        // 检查是否有 src 目录
        let src_info = if working_dir.join("src").is_dir() {
            "\n注意：存在 src/ 目录".to_string()
        } else {
            String::new()
        };

        format!(
            r#"当前工作目录：{}
目录内容：
{}
{}{}{}"#,
            dir_path,
            entries_str,
            build_info,
            src_info,
            if entries.len() > 20 {
                "\n(只显示前20项)"
            } else {
                ""
            }
        )
    }

    /// 格式化工具定义，包含完整参数 schema
    fn format_tools_with_schemas(&self, tools_defs: &[crate::types::Tool]) -> String {
        tools_defs
            .iter()
            .map(|t| {
                let name = &t.function.name;
                let description = &t.function.description;
                let params = &t.function.parameters;

                // 提取 parameters properties 作为参数说明
                let params_desc = if let Some(props) = params.get("properties") {
                    if let Some(obj) = props.as_object() {
                        obj.iter()
                            .map(|(param_name, param_info)| {
                                // 获取参数类型描述
                                let param_type = param_info
                                    .get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("any");
                                let param_desc = param_info
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let enum_vals = param_info.get("enum").and_then(|v| {
                                    v.as_array().map(|arr| {
                                        arr.iter()
                                            .filter_map(|x| x.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                });
                                let enum_str = enum_vals
                                    .map(|e| format!(" (可选值: {})", e))
                                    .unwrap_or_default();
                                format!(
                                    "  - {}: {}（类型：{}{}）",
                                    param_name, param_desc, param_type, enum_str
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                if params_desc.is_empty() {
                    format!("### {}\n{}\n(无参数)", name, description)
                } else {
                    format!("### {}\n{}\n参数：\n{}", name, description, params_desc)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
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
            required_tools: Option<Vec<String>>,
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
            sub_goals.push(SubGoal {
                goal_id: sg.goal_id,
                description: sg.description,
                priority: sg.priority.unwrap_or(0),
                depends_on: sg.depends_on.unwrap_or_default(),
                required_tools: sg.required_tools.unwrap_or_default(),
            });
        }

        let summary = format!("Decomposed into {} sub-goals", sub_goals.len());

        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: decomposed into {} sub_goals, strategy={:?}",
            sub_goals.len(),
            execution_strategy
        );
        for (i, sg) in sub_goals.iter().enumerate() {
            log::info!(
                target: "crabmate",
                "[HIERARCHICAL]   goal[{}]: id={} desc={}",
                i,
                sg.goal_id,
                truncate_task(&sg.description)
            );
        }

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

/// 从响应中提取 JSON（处理 LLM 输出中可能存在的多余字符）
fn extract_json(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    // 使用括号计数找到匹配的闭括号，处理嵌套 JSON
    let mut depth = 0;
    for (i, c) in content[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&content[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// 截断任务字符串用于日志（按字符边界截断，支持中文）
fn truncate_task(task: &str) -> String {
    const MAX_LEN: usize = 100;
    if task.len() > MAX_LEN {
        let truncated = task
            .char_indices()
            .take(MAX_LEN - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &task[..truncated])
    } else {
        task.to_string()
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
