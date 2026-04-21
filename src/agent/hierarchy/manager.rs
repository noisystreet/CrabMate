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

/// Manager 对失败子目标的决策
#[derive(Debug)]
pub enum ManagerDecision {
    /// 重新执行修正后的子目标
    Retry { updated_goal: Box<SubGoal> },
    /// 跳过该子目标
    Skip { reason: String },
    /// 终止整个任务
    Abort { reason: String },
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
                let output = self.parse_output(&content)?;

                // 如果 LLM 返回空子目标列表，使用 fallback 确保至少有一个任务执行
                if output.sub_goals.is_empty() {
                    log::warn!(target: "crabmate", "[HIERARCHICAL] Manager: LLM returned empty sub_goals, using fallback");
                    Ok(self.decompose_fallback(task))
                } else {
                    Ok(output)
                }
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

    /// 处理失败的子目标，返回决策
    ///
    /// 当 Executor 执行子目标失败时，调用此方法让 Manager 分析失败原因，
    /// 决定是重试（可能修改子目标描述）、跳过还是终止。
    #[allow(clippy::too_many_arguments)]
    pub async fn handle_failed_goal(
        &self,
        failed_goal: &SubGoal,
        error_message: &str,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        previous_artifacts: &[super::task::Artifact],
    ) -> Result<ManagerDecision, ManagerError> {
        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: handling failed goal={} error={}",
            failed_goal.goal_id,
            truncate_for_log(error_message, 200)
        );

        let prompt = self.build_failed_goal_prompt(
            failed_goal,
            error_message,
            working_dir,
            tools_defs,
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
                self.parse_failure_decision(&content, failed_goal)
            }
            Err(e) => {
                log::warn!(target: "crabmate", "Manager handle_failed_goal LLM call failed: {}", e);
                // 默认跳过
                Ok(ManagerDecision::Skip {
                    reason: format!("Manager unavailable: {}", e),
                })
            }
        }
    }

    /// 构建处理失败目标的 prompt
    fn build_failed_goal_prompt(
        &self,
        failed_goal: &SubGoal,
        error_message: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        previous_artifacts: &[super::task::Artifact],
    ) -> String {
        let workspace_context = self.get_workspace_context(working_dir);
        let artifacts_summary = self.format_artifacts_summary(previous_artifacts);

        format!(
            r#"## 任务
你是一个任务执行协调专家。子目标执行失败，需要你决定如何处理。

原始子目标：{}
目标描述：{}

执行失败信息：
{}

## 当前工作目录
{}
重要：基于实际文件状态做决策。
**禁止假设**任何文件或目录存在，必须先通过 read_dir 确认。

## 已有的产物（可供参考）
{}

## 工具定义（完整参数 schema）
{}
重要：
- `run_command` 需要分别指定 `command`（命令名如 `ls`, `gcc`）和 `args`（参数数组），不要合并

## 决策要求

分析失败原因，从以下选项中选择：

1. **重试（Retry）**：如果失败是因为工具参数错误或描述不准确，返回修改后的子目标。
   - JSON 格式：{{"decision": "retry", "updated_goal": {{"goal_id": "goal_1", "description": "修改后的描述", "priority": 1, "depends_on": [], "required_tools": ["tool1"]}}}}

2. **跳过（Skip）**：如果失败是因为条件不满足或无法完成，标记跳过并提供原因。
   - JSON 格式：{{"decision": "skip", "reason": "跳过原因"}}

3. **终止（Abort）**：如果任务根本无法完成，终止整个任务。
   - JSON 格式：{{"decision": "abort", "reason": "终止原因"}}

## 输出格式
只输出 JSON，不要有任何解释文字。
"#,
            failed_goal.goal_id,
            failed_goal.description,
            error_message,
            workspace_context,
            artifacts_summary,
            self.format_tools_with_schemas(tools_defs)
        )
    }

    /// 解析失败决策
    fn parse_failure_decision(
        &self,
        content: &str,
        original_goal: &SubGoal,
    ) -> Result<ManagerDecision, ManagerError> {
        let json_str = extract_json(content).ok_or_else(|| {
            ManagerError::ParseError("Failed to extract JSON from response".to_string())
        })?;

        #[derive(serde::Deserialize)]
        struct DecisionJson {
            decision: String,
            updated_goal: Option<SubGoalJson>,
            reason: Option<String>,
        }

        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct SubGoalJson {
            goal_id: String,
            description: String,
            priority: Option<u32>,
            depends_on: Option<Vec<String>>,
            required_tools: Option<Vec<String>>,
            #[serde(default)]
            goal_type: Option<super::task::GoalType>,
        }

        let parsed: DecisionJson =
            serde_json::from_str(json_str).map_err(|e| ManagerError::ParseError(e.to_string()))?;

        match parsed.decision.as_str() {
            "retry" => {
                if let Some(ug) = parsed.updated_goal {
                    Ok(ManagerDecision::Retry {
                        updated_goal: Box::new(SubGoal {
                            goal_id: ug.goal_id,
                            description: ug.description,
                            priority: ug.priority.unwrap_or(original_goal.priority),
                            depends_on: ug.depends_on.unwrap_or_default(),
                            required_tools: ug.required_tools.unwrap_or_default(),
                            goal_type: original_goal.goal_type.clone(),
                            build_requirements: original_goal.build_requirements.clone(),
                            acceptance: None,
                            max_retries: None,
                        }),
                    })
                } else {
                    Ok(ManagerDecision::Retry {
                        updated_goal: Box::new(original_goal.clone()),
                    })
                }
            }
            "skip" => Ok(ManagerDecision::Skip {
                reason: parsed
                    .reason
                    .unwrap_or_else(|| "Skipped by manager".to_string()),
            }),
            "abort" => Ok(ManagerDecision::Abort {
                reason: parsed
                    .reason
                    .unwrap_or_else(|| "Aborted by manager".to_string()),
            }),
            _ => Ok(ManagerDecision::Skip {
                reason: format!("Unknown decision: {}", parsed.decision),
            }),
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
重要：
- 子目标的描述必须基于**实际存在的**文件和目录
- **禁止假设**任何文件或目录存在，必须先通过 read_dir 确认
- **禁止在子目标描述中使用不存在的路径**，如 `src/`、`include/`、`build/` 等，除非已确认存在
- 如果需要读取某个目录，**必须先用 read_dir 确认它存在**，才能在后续子目标中使用
- 如果需要操作某个文件（如 search_replace、modify_file），**必须先确认文件存在**

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
- **`create_file` 是创建文件的唯一正确方式**，禁止使用 `echo`、`cat`、`tee` 等命令通过 `run_command` 创建文件
- `create_file` 的 `content` 参数：在 JSON 中必须使用正确的转义序列：换行用 `\n`，制表用 `\t`，双引号用 `\"`
- `run_command` 需要分别指定 `command`（命令名如 `ls`, `gcc`）和 `args`（参数数组），不要合并
- `run_command` 可以执行工作区内的 `./xxx` 形式可执行文件（如 `./build/app`）
- `read_dir` 用于读取目录内容，路径必须是相对路径且不能包含 `..`

## CMake 项目特殊规则
- 如果工作目录包含 CMakeLists.txt，**必须使用**其中定义的可执行文件目标名称
- **禁止假设**可执行文件名称（如 demo、test、main 等），必须使用 CMakeLists.txt 中 `add_executable()` 定义的实际名称
- 运行可执行文件时，路径必须是 `./build/<实际名称>` 或 `./<实际名称>`，不能使用假设的名称

## Cargo/Rust 项目特殊规则
- 如果执行了 `cargo init` 且创建了子目录（如 `tmp/`），后续所有 `cargo` 命令必须在那个子目录中执行
- 使用 `run_command` 执行 cargo 命令前，**必须先 `cd` 到项目目录**：`{{"command": "cd", "args": ["tmp"]}}`，然后再执行 `{{"command": "cargo", "args": ["build"]}}`
- **禁止假设**可执行文件名称，必须使用 `Cargo.toml` 中 `[[bin]]` 或默认的 `src/main.rs` 对应的名称
- 运行 Rust 可执行文件时，路径必须是 `./target/debug/<名称>`（在子目录内）或 `./tmp/target/debug/<名称>`（从根目录）

## 输出格式
**必须输出标准 JSON 格式**，不要输出任何其他内容。JSON 必须符合以下结构：
```
{{{{
    "sub_goals": [
        {{
            "goal_id": "goal_1",
            "description": "子目标描述（基于实际文件结构和已有产物）",
            "priority": 1,
            "depends_on": ["goal_id_of_dependency"],
            "required_tools": ["tool_name1", "tool_name2"],
            "goal_type": "fix"  // 或 "analyze"
        }}
    ],
    "execution_strategy": "hybrid"
}}}}
```
- `goal_id` 必须是字符串
- `description` 必须是字符串
- `priority` 必须是数字
- `depends_on` 必须是字符串数组
- `required_tools` 必须是字符串数组
- `goal_type` 必须是 `"fix"`（修复/执行）或 `"analyze"`（分析/收集）。如果只需要收集信息（如编译错误），用 `"analyze"`，失败后直接跳过。

## 约束
- 子目标数量不超过 {}
- **只输出 JSON，不要有markdown代码块标记、不要有任何解释文字**
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
重要：
- 子目标的描述必须基于**实际存在的**文件和目录
- **禁止假设**任何文件或目录存在，必须先通过 read_dir 确认
- **禁止在子目标描述中使用不存在的路径**，如 `src/`、`include/`、`build/` 等，除非已确认存在
- 如果需要读取某个目录，**必须先用 read_dir 确认它存在**，才能在后续子目标中使用
- 如果需要操作某个文件（如 search_replace、modify_file），**必须先确认文件存在**

## 工具定义（完整参数 schema）
{}
重要：
- 分配工具时，确保工具参数能匹配子目标需求
- `run_command` 需要分别指定 `command`（命令名如 `ls`, `gcc`）和 `args`（参数数组），不要合并
- `run_command` 可以执行工作区内的 `./xxx` 形式可执行文件（如 `./build/app`）
- `read_dir` 用于读取目录内容，路径必须是相对路径且不能包含 `..`
- `create_file` 的 `content` 参数必须是正确的 JSON 字符串（特殊字符需要转义）

## 输出格式
**必须输出标准 JSON 格式**，不要输出任何其他内容。JSON 必须符合以下结构：
```
{{{{
    "sub_goals": [
        {{
            "goal_id": "goal_1",
            "description": "子目标描述（基于实际文件结构）",
            "priority": 1,
            "depends_on": [],
            "required_tools": ["tool_name1", "tool_name2"],
            "goal_type": "fix"  // 或 "analyze"
        }}
    ],
    "execution_strategy": "hybrid"
}}}}
```
- `goal_id` 必须是字符串
- `description` 必须是字符串
- `priority` 必须是数字
- `depends_on` 必须是字符串数组
- `required_tools` 必须是字符串数组
- `goal_type` 必须是 `"fix"`（修复/执行）或 `"analyze"`（分析/收集）。如果只需要收集信息（如编译错误），用 `"analyze"`，失败后直接跳过。

## 约束
- 子目标数量不超过 {}
- **只输出 JSON，不要有markdown代码块标记、不要有任何解释文字**
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

        // 解析 CMakeLists.txt 获取可执行文件名称
        let cmake_info = self.parse_cmake_info(working_dir);

        format!(
            r#"当前工作目录：{}
目录内容：
{}{}{}{}{}"#,
            dir_path,
            entries_str,
            build_info,
            src_info,
            cmake_info,
            if entries.len() > 20 {
                "\n(只显示前20项)"
            } else {
                ""
            }
        )
    }

    /// 解析 CMakeLists.txt 获取项目信息
    fn parse_cmake_info(&self, working_dir: &std::path::Path) -> String {
        let cmake_path = working_dir.join("CMakeLists.txt");
        if !cmake_path.exists() {
            return String::new();
        }

        let content = match std::fs::read_to_string(&cmake_path) {
            Ok(c) => c,
            Err(_) => return String::new(),
        };

        let mut info_parts = Vec::new();

        // 解析项目名称
        if let Some(project_name) = self.extract_cmake_project_name(&content) {
            info_parts.push(format!("CMake 项目名称: {}", project_name));
        }

        // 解析可执行文件目标
        let executables = self.extract_cmake_executables(&content);
        if !executables.is_empty() {
            info_parts.push(format!("CMake 可执行文件目标: {}", executables.join(", ")));
        }

        if info_parts.is_empty() {
            String::new()
        } else {
            format!("\nCMake 项目信息:\n  - {}", info_parts.join("\n  - "))
        }
    }

    /// 从 CMakeLists.txt 内容中提取项目名称
    fn extract_cmake_project_name(&self, content: &str) -> Option<String> {
        // 匹配 project(Name) 或 project(Name VERSION x.y.z)
        let re = regex::Regex::new(r"project\s*\(\s*([A-Za-z0-9_]+)").ok()?;
        re.captures(content)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// 从 CMakeLists.txt 内容中提取可执行文件目标
    fn extract_cmake_executables(&self, content: &str) -> Vec<String> {
        let mut executables = Vec::new();

        // 匹配 add_executable(name source1 source2 ...)
        let re = regex::Regex::new(r"add_executable\s*\(\s*([A-Za-z0-9_]+)").ok();
        if let Some(re) = re {
            for cap in re.captures_iter(content) {
                if let Some(name) = cap.get(1) {
                    executables.push(name.as_str().to_string());
                }
            }
        }

        executables
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
            log::warn!(target: "crabmate", "[HIERARCHICAL] Manager: failed to extract JSON from response: {:?}", truncate_for_log(content, 200));
            ManagerError::ParseError("Failed to extract JSON from response".to_string())
        })?;

        log::debug!(target: "crabmate", "[HIERARCHICAL] Manager: parsing JSON: {}", truncate_for_log(json_str, 500));

        #[derive(serde::Deserialize)]
        struct OutputJson {
            sub_goals: Vec<SubGoalJson>,
            execution_strategy: Option<String>,
        }

        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct SubGoalJson {
            goal_id: String,
            description: String,
            priority: Option<u32>,
            depends_on: Option<Vec<String>>,
            required_tools: Option<Vec<String>>,
            #[serde(default)]
            goal_type: Option<super::task::GoalType>,
        }

        let parsed: OutputJson =
            serde_json::from_str(json_str).map_err(|e| {
                log::warn!(target: "crabmate", "[HIERARCHICAL] Manager: JSON parse error: {} for content: {}", e, truncate_for_log(json_str, 300));
                ManagerError::ParseError(e.to_string())
            })?;

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
                goal_type: sg.goal_type.unwrap_or_default(),
                build_requirements: super::task::BuildRequirements::default(),
                acceptance: None,
                max_retries: None,
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

    /// 反思验证失败并重新规划子目标
    ///
    /// 当验证失败时，分析失败原因并生成修复策略
    #[allow(clippy::too_many_arguments)]
    pub async fn reflect_and_replan(
        &self,
        failed_goal: &SubGoal,
        verification_failure: &str,
        execution_result: &TaskResult,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        artifacts: &[super::task::Artifact],
    ) -> Result<SubGoal, ManagerError> {
        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: reflecting on verification failure for goal={}",
            failed_goal.goal_id
        );

        let prompt = self.build_reflection_prompt(
            failed_goal,
            verification_failure,
            execution_result,
            working_dir,
            tools_defs,
            artifacts,
        );

        let messages = vec![Message::user_only(&prompt)];
        let request =
            no_tools_chat_request(cfg, &messages, None, None, LlmSeedOverride::FromConfig);

        let transport_opts = LlmRetryingTransportOpts::headless_no_stream();
        let params = CompleteChatRetryingParams::new(
            llm_backend,
            client,
            api_key,
            cfg,
            transport_opts,
            None,
            None,
        );
        let response = complete_chat_retrying(&params, &request)
            .await
            .map_err(|e| ManagerError::LlmError(e.to_string()))?;

        let content = message_content_as_str(&response.0.content).unwrap_or_default();
        log::debug!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: reflection response: {}",
            truncate_for_log(content, 500)
        );

        // 解析更新的子目标
        self.parse_reflection_output(failed_goal, content)
    }

    /// 构建反思 prompt
    fn build_reflection_prompt(
        &self,
        failed_goal: &SubGoal,
        verification_failure: &str,
        execution_result: &TaskResult,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        artifacts: &[super::task::Artifact],
    ) -> String {
        let workspace_context = self.get_workspace_context(working_dir);
        let tools_description = self.format_tools_with_schemas(tools_defs);
        let artifacts_summary = self.format_artifacts_summary(artifacts);

        let execution_output = execution_result.output.as_deref().unwrap_or("(无输出)");
        let execution_error = execution_result.error.as_deref().unwrap_or("(无错误)");

        format!(
            r#"## 任务
你是一个任务修复专家。子目标执行后验证失败，需要分析原因并生成修复策略。

## 失败的子目标
- goal_id: {}
- description: {}
- goal_type: {:?}
- required_tools: {:?}

## 验证失败原因
{}

## 执行输出
```
{}
```

## 执行错误
```
{}
```

## 工作目录上下文
{}

## 已完成的产物
{}

## 工具定义
{}

## 输出格式
**必须输出标准 JSON 格式**，不要输出任何其他内容。JSON 必须符合以下结构：
```json
{{
    "analysis": "失败原因分析（简要说明为什么验证失败）",
    "fix_strategy": "修复策略（简要说明如何修复）",
    "updated_goal": {{
        "goal_id": "{}",
        "description": "更新后的子目标描述（更具体、包含修复步骤）",
        "priority": {},
        "depends_on": {:?},
        "required_tools": {:?},
        "goal_type": "{:?}",
        "acceptance": {{
            "expect_file_exists": ["文件路径1", "文件路径2"],
            "expect_command_success": "可选的验证命令",
            "expect_output_contains": ["期望的输出片段"],
            "expect_exit_code": 0
        }},
        "max_retries": 3
    }}
}}
```

重要：
1. `updated_goal.goal_id` 必须与原子目标相同
2. `updated_goal.description` 应该更具体，明确包含修复步骤
3. 添加或完善 `acceptance` 条件，确保下次能正确验证
4. `max_retries` 控制验证失败后的重试次数
"#,
            failed_goal.goal_id,
            failed_goal.description,
            failed_goal.goal_type,
            failed_goal.required_tools,
            verification_failure,
            execution_output,
            execution_error,
            workspace_context,
            artifacts_summary,
            tools_description,
            failed_goal.goal_id,
            failed_goal.priority,
            failed_goal.depends_on,
            failed_goal.required_tools,
            failed_goal.goal_type,
        )
    }

    /// 解析反思输出
    fn parse_reflection_output(
        &self,
        original_goal: &SubGoal,
        content: &str,
    ) -> Result<SubGoal, ManagerError> {
        let json_str = extract_json(content).ok_or_else(|| {
            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: failed to extract JSON from reflection response"
            );
            ManagerError::ParseError("Failed to extract JSON from reflection response".to_string())
        })?;

        #[derive(serde::Deserialize)]
        struct ReflectionOutput {
            analysis: String,
            fix_strategy: String,
            updated_goal: SubGoalJson,
        }

        #[derive(serde::Deserialize)]
        struct SubGoalJson {
            goal_id: String,
            description: String,
            priority: Option<u32>,
            depends_on: Option<Vec<String>>,
            required_tools: Option<Vec<String>>,
            #[serde(default)]
            goal_type: Option<super::task::GoalType>,
            #[serde(default)]
            acceptance: Option<super::task::GoalAcceptance>,
            #[serde(default)]
            max_retries: Option<usize>,
        }

        let parsed: ReflectionOutput = serde_json::from_str(json_str).map_err(|e| {
            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: JSON parse error in reflection: {}",
                e
            );
            ManagerError::ParseError(e.to_string())
        })?;

        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: reflection analysis: {}",
            parsed.analysis
        );
        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: fix strategy: {}",
            parsed.fix_strategy
        );

        // 确保 goal_id 一致
        if parsed.updated_goal.goal_id != original_goal.goal_id {
            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: reflection changed goal_id from {} to {}, using original",
                original_goal.goal_id,
                parsed.updated_goal.goal_id
            );
        }

        let updated_goal = SubGoal {
            goal_id: original_goal.goal_id.clone(),
            description: parsed.updated_goal.description,
            priority: parsed
                .updated_goal
                .priority
                .unwrap_or(original_goal.priority),
            depends_on: parsed
                .updated_goal
                .depends_on
                .unwrap_or_else(|| original_goal.depends_on.clone()),
            required_tools: parsed
                .updated_goal
                .required_tools
                .unwrap_or_else(|| original_goal.required_tools.clone()),
            goal_type: parsed
                .updated_goal
                .goal_type
                .unwrap_or(original_goal.goal_type.clone()),
            build_requirements: original_goal.build_requirements.clone(),
            acceptance: parsed.updated_goal.acceptance,
            max_retries: parsed
                .updated_goal
                .max_retries
                .or(original_goal.max_retries),
        };

        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: reflection produced updated goal with acceptance={}",
            updated_goal.acceptance.is_some()
        );

        Ok(updated_goal)
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
    truncate_for_log(task, 100)
}

/// 按字符边界截断字符串（支持中文），用于日志输出
fn truncate_for_log(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars - 3).collect();
    format!("{}...", truncated)
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
