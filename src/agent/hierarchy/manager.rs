//! Manager Agent：任务分解与协调
//!
//! Manager 负责：
//! - 理解高层任务目标
//! - 分解为可执行的 SubGoals
//! - 确定执行策略
//! - 协调子目标执行
//! - 会话状态管理（避免重复执行）

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{
    CompleteChatRetryingParams, LlmRetryingTransportOpts, complete_chat_retrying,
    no_tools_chat_request_for_hierarchical_manager,
};
use crate::types::{LlmSeedOverride, Message, message_content_as_str};

use super::session_state::SessionStateManager;
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
    /// 会话状态管理器（可选）
    session_manager: Option<std::sync::Arc<SessionStateManager>>,
}

impl ManagerAgent {
    /// Manager 分解/重规划输出的强约束 schema（提示词用）。
    fn manager_output_schema_contract() -> &'static str {
        r#"{
  "type": "object",
  "required": ["sub_goals", "execution_strategy"],
  "additionalProperties": false,
  "properties": {
    "sub_goals": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["goal_id", "description", "priority", "depends_on", "required_tools", "goal_type"],
        "additionalProperties": false,
        "properties": {
          "goal_id": {"type": "string"},
          "description": {"type": "string"},
          "priority": {"type": "integer", "minimum": 0},
          "depends_on": {"type": "array", "items": {"type": "string"}},
          "required_tools": {"type": "array", "items": {"type": "string"}},
          "goal_type": {"type": "string", "enum": ["fix", "analyze"]},
          "build_requirements": {
            "type": "object",
            "properties": {
              "needs_artifacts": {"type": "array", "items": {"type": "string", "enum": ["SourceFile", "ObjectFile", "Executable", "StaticLibrary", "DynamicLibrary", "BuildLog"]}},
              "produces_artifacts": {"type": "array", "items": {"type": "string", "enum": ["SourceFile", "ObjectFile", "Executable", "StaticLibrary", "DynamicLibrary", "BuildLog"]}}
            }
          }
        }
      }
    },
    "execution_strategy": {"type": "string", "enum": ["sequential", "parallel", "hybrid"]}
  }
}"#
    }

    /// Manager 的结构化 JSON 调用：禁用 thinking/reasoning_split，避免 DeepSeek thinking 模式
    /// 对历史 `reasoning_content` 回传的额外约束干扰纯 JSON 分解/修复流程。
    fn force_manager_structured_json_mode(req: &mut crate::types::ChatRequest) {
        req.thinking = None;
        req.reasoning_split = None;
    }

    pub fn new(config: ManagerConfig) -> Self {
        Self {
            config,
            session_manager: None,
        }
    }

    /// 设置会话状态管理器
    pub fn with_session_manager(
        mut self,
        session_manager: std::sync::Arc<SessionStateManager>,
    ) -> Self {
        self.session_manager = Some(session_manager);
        self
    }

    /// 检查任务是否需要执行（基于会话状态）
    fn should_execute_task(&self, task: &str) -> bool {
        if let Some(ref manager) = self.session_manager {
            manager.should_execute_task(task)
        } else {
            true // 没有会话管理器时，默认执行
        }
    }

    /// 检查可执行文件是否已构建
    fn is_executable_built(&self, name: &str) -> Option<std::path::PathBuf> {
        self.session_manager
            .as_ref()
            .and_then(|m| m.is_executable_built(name))
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

        // 检查任务是否已完成（会话状态检查）
        if !self.should_execute_task(task) {
            log::info!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: task '{}' already completed, returning empty plan",
                task
            );
            return Ok(ManagerOutput {
                sub_goals: vec![],
                execution_strategy: ExecutionStrategy::Sequential,
                summary: format!("Task '{}' already completed in previous session", task),
            });
        }

        // 检查是否是编译类任务且产物已存在
        if self.is_compile_task(task)
            && let Some(exe_path) = self.check_existing_executable(task)
        {
            log::info!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: executable already exists at {:?}, skipping compilation",
                exe_path
            );
            return Ok(ManagerOutput {
                sub_goals: vec![],
                execution_strategy: ExecutionStrategy::Sequential,
                summary: format!(
                    "Executable already exists at {:?}, compilation skipped",
                    exe_path
                ),
            });
        }

        let prompt = self.build_decomposition_prompt(task, working_dir, tools_defs);

        let messages = vec![Message::user_only(&prompt)];
        let mut request = no_tools_chat_request_for_hierarchical_manager(
            cfg,
            &messages,
            None,
            None,
            LlmSeedOverride::FromConfig,
        );
        Self::force_manager_structured_json_mode(&mut request);

        match complete_chat_retrying(
            &CompleteChatRetryingParams::new(
                llm_backend,
                client,
                api_key,
                cfg,
                LlmRetryingTransportOpts::headless_no_stream(),
                None,
                None,
            ),
            &request,
        )
        .await
        {
            Ok((response, finish_reason)) => {
                let content = message_content_as_str(&response.content)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let output = self
                    .parse_output_with_one_json_repair(
                        &content,
                        Some(finish_reason.as_str()),
                        cfg,
                        llm_backend,
                        client,
                        api_key,
                    )
                    .await?;

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
        let mut request = no_tools_chat_request_for_hierarchical_manager(
            cfg,
            &messages,
            None,
            None,
            LlmSeedOverride::FromConfig,
        );
        Self::force_manager_structured_json_mode(&mut request);

        match complete_chat_retrying(
            &CompleteChatRetryingParams::new(
                llm_backend,
                client,
                api_key,
                cfg,
                LlmRetryingTransportOpts::headless_no_stream(),
                None,
                None,
            ),
            &request,
        )
        .await
        {
            Ok((response, finish_reason)) => {
                let content = message_content_as_str(&response.content)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                self.parse_output_with_one_json_repair(
                    &content,
                    Some(finish_reason.as_str()),
                    cfg,
                    llm_backend,
                    client,
                    api_key,
                )
                .await
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
        let mut request = no_tools_chat_request_for_hierarchical_manager(
            cfg,
            &messages,
            None,
            None,
            LlmSeedOverride::FromConfig,
        );
        Self::force_manager_structured_json_mode(&mut request);

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
            Ok((response, _finish_reason)) => {
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
{}

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
            self.format_tools_with_schemas(tools_defs),
            Self::manager_tool_invariants(),
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
            #[serde(default)]
            consumes_from_dependencies: Option<Vec<super::task::DependencyContractEntry>>,
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
                            consumes_from_dependencies: ug
                                .consumes_from_dependencies
                                .unwrap_or_default(),
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

## 分解硬性规则（必须遵守）
1. 子目标必须**单一职责**，禁止跨步执行。
2. 每个子目标只允许执行其描述中的动作，禁止提前执行后续步骤。
3. 产物命名必须全程一致；一旦确定名称（如可执行文件名），后续不得改名或混用。
4. `depends_on` 必须准确，后续步骤不得绕过依赖。
5. 每个子目标必须给出可验证的 I/O 与验收目标，且与本步职责严格匹配。
6. 若某步是“创建文件”，描述中只允许文件创建/内容核验，不得包含配置、编译或运行动作。
7. 若某步是“配置构建”，描述中只允许配置动作，不得包含编译或运行动作。
8. 若某步是“编译”，描述中只允许编译与产物核验，不得运行程序。
9. 若某步是“运行验证”，描述中只允许运行与输出核验。
10. 若任务涉及 C++ + CMake，默认采用稳定链路：检查目录 → 写 `main.cpp` → 写 `CMakeLists.txt` → `cmake -S . -B build` → `cmake --build build` → 运行产物；且可执行文件名需在 `CMakeLists.txt` 与后续子目标中保持一致（例如 `myapp`）。

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
{}

## CMake 项目特殊规则
- 如果工作目录包含 CMakeLists.txt，**必须使用**其中定义的可执行文件目标名称
- **禁止假设**可执行文件名称（如 demo、test、main 等），必须使用 CMakeLists.txt 中 `add_executable()` 定义的实际名称
- **禁止**在源码树根部使用 `file(GLOB_RECURSE "*.cpp" …)` 且不排除 `build/`、`CMakeFiles/`：会把 CMake 生成的 `CompilerId*.c/cpp` 等编进同一可执行目标，链接报 **multiple definition of `main`**。简单项目请 **`add_executable(目标名 main.cpp)`** 显式列源；若必须用 GLOB，须**排除** `build` 与 `CMakeFiles` 目录
- 运行构建产物时，**优先**用 **`run_executable` + 工作区相对路径**（如经 `read_dir` 在 `build/` 中确认后的 `build/<目标名>`）；不要用猜测的名称或错误的 JSON `args` 等「凑合」方式跳过验证
- 凡子目标描述含 **cmake / 编译 / make / 构建 / 检查 build / 验证可执行** 等，且你填写了非空 `required_tools`，**必须**包含 **`run_command`**（以及需要直接跑产物时的 **`run_executable`**）；仅 `read_dir` 会导致无法执行 `cmake --build` 等而空转

## Cargo/Rust 项目特殊规则
- 如果执行了 `cargo init` 且创建了子目录（如 `tmp/`），后续所有 `cargo` 命令必须在那个子目录中执行
- 使用 `run_command` 执行 cargo 命令前，**必须先 `cd` 到项目目录**：`{{"command": "cd", "args": ["tmp"]}}`，然后再执行 `{{"command": "cargo", "args": ["build"]}}`
- **禁止假设**可执行文件名称，必须使用 `Cargo.toml` 中 `[[bin]]` 或默认的 `src/main.rs` 对应的名称
- 运行 Rust 可执行文件时，路径必须是 `./target/debug/<名称>`（在子目录内）或 `./tmp/target/debug/<名称>`（从根目录）

## 子目标 I/O 契约
- 同「初次分解」：每个子目标 `description` 写清 I/O；`depends_on` + `consumes_from_dependencies` + 可选 `build_requirements`；在描述与工具里用 `{{ref:<前序id>:<artifact_id>}}` 或 `{{artifact:...}}`，**不要**写绝对路径。

## 输出格式
**必须输出标准 JSON 格式**，不要输出任何其他内容。JSON 必须符合以下结构：
```
{{{{
    "sub_goals": [
        {{{{
            "goal_id": "goal_1",
            "description": "（I/O 契约）子目标描述（基于实际文件结构和已有产物）",
            "priority": 1,
            "depends_on": ["goal_id_of_dependency"],
            "consumes_from_dependencies": [
                {{"from_goal_id": "goal_id_of_dependency", "only_kinds": null}}
            ],
            "build_requirements": {{"needs_artifacts": [], "produces_artifacts": []}},
            "required_tools": ["tool_name1", "tool_name2"],
            "goal_type": "fix"  // 或 "analyze"
        }}}}
    ],
    "execution_strategy": "hybrid"
}}}}
```
- `goal_id` 必须是字符串
- `description` 必须是字符串
- `priority` 必须是数字
- `depends_on` 必须是字符串数组
- `consumes_from_dependencies` 可选，规则同初次分解
- `build_requirements` 可选
- `required_tools` 必须是字符串数组
- `goal_type` 必须是 `"fix"`（修复/执行）或 `"analyze"`（分析/收集）。如果只需要收集信息（如编译错误），用 `"analyze"`，失败后直接跳过。

## 强约束 Schema（必须满足）
```json
{}
```

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
            Self::manager_tool_invariants(),
            Self::manager_output_schema_contract(),
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

        // 识别任务类型并添加特定指导
        let task_type_guidance = self.get_task_type_guidance(task);

        format!(
            r#"## 任务
你是一个任务分解专家。请将以下用户任务分解为可执行的子目标。

任务：{}

## 任务类型识别与指导
{}

## 分解硬性规则（必须遵守）
1. 子目标必须**单一职责**，禁止跨步执行。
2. 每个子目标只允许执行其描述中的动作，禁止提前执行后续步骤。
3. 产物命名必须全程一致；一旦确定名称（如可执行文件名），后续不得改名或混用。
4. `depends_on` 必须准确，后续步骤不得绕过依赖。
5. 每个子目标必须给出可验证的 I/O 与验收目标，且与本步职责严格匹配。
6. 若某步是“创建文件”，描述中只允许文件创建/内容核验，不得包含配置、编译或运行动作。
7. 若某步是“配置构建”，描述中只允许配置动作，不得包含编译或运行动作。
8. 若某步是“编译”，描述中只允许编译与产物核验，不得运行程序。
9. 若某步是“运行验证”，描述中只允许运行与输出核验。
10. 若任务涉及 C++ + CMake，默认采用稳定链路：检查目录 → 写 `main.cpp` → 写 `CMakeLists.txt` → `cmake -S . -B build` → `cmake --build build` → 运行产物；且可执行文件名需在 `CMakeLists.txt` 与后续子目标中保持一致（例如 `myapp`）。

## 子目标 I/O 契约（必须显式写清，便于层间传产物与注入裁剪）
- 对**每个**子目标在 `description` 开头用 2～4 行写清：本步**输入/依赖**、本步**预期输出**（路径或行为）；若依赖前序子目标，须写进 `depends_on`。
- `consumes_from_dependencies`：列出本步**实际消费**的前序 `goal_id`；`only_kinds` 可选，用于**注入到 Operator 的依赖上下文**的裁剪：省略或 `[]` 表示**默认**（不注入冗长 `buildlog` 与纯长文本 `commandoutput`）；填 `["all"]` 或 `["any"]` 表示不筛类型；否则为子串匹配，与产物类型字符串（如 `buildartifact(executable)`、`file`）不区分大小写子串匹配，例如 `["source"]`、`["executable"]`。
- `build_requirements` 可选；编译类任务可填 `needs_artifacts` / `produces_artifacts`（`SourceFile` / `ObjectFile` / `Executable` 等）。
- 在 `description` 与工具设计里，引用前序**具体文件/构建物**时优先写 **`{{ref:<前序子目标id>:<artifact_id>}}`** 或 `{{artifact:文件名.stem}}`；**不要**在 JSON 中写本机**绝对**路径。执行时 `{{ref:...}}` 会展开为工作区相对 path。

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
{}

## 输出格式
**必须输出标准 JSON 格式**，不要输出任何其他内容。JSON 必须符合以下结构：
```{{{{
    "sub_goals": [
        {{{{
            "goal_id": "goal_1",
            "description": "（I/O: 输入/输出/产物）子目标描述（基于实际文件结构）",
            "priority": 1,
            "depends_on": ["goal_0"],
            "consumes_from_dependencies": [
                {{"from_goal_id": "goal_0", "only_kinds": null}}
            ],
            "build_requirements": {{"needs_artifacts": ["SourceFile"], "produces_artifacts": ["Executable"]}},
            "required_tools": ["tool_name1", "tool_name2"],
            "goal_type": "fix"  // 或 "analyze"
        }}}}
    ],
    "execution_strategy": "hybrid"
}}}}
```
- `goal_id` 必须是字符串
- `description` 必须是字符串
- `priority` 必须是数字
- `depends_on` 必须是字符串数组
- `consumes_from_dependencies` 是可选数组，每项为 `from_goal_id` 字符串 + 可选 `only_kinds: string[] | null`（`from_goal_id` 必须出现在 `depends_on` 中；空数组可省略本字段，由执行器在可行时**自动补全**并默认裁剪类型）
- `build_requirements` 可选
- `required_tools` 必须是字符串数组
- `goal_type` 必须是 `"fix"`（修复/执行）或 `"analyze"`（分析/收集）。如果只需要收集信息（如编译错误），用 `"analyze"`，失败后直接跳过。

## 强约束 Schema（必须满足）
```json
{}
```

## 约束
- 子目标数量不超过 {}
- **只输出 JSON，不要有markdown代码块标记、不要有任何解释文字**
"#,
            task,
            task_type_guidance,
            workspace_context,
            tools_description,
            Self::manager_tool_invariants(),
            Self::manager_output_schema_contract(),
            self.config.max_sub_goals
        )
    }

    /// 根据任务描述识别任务类型并返回特定指导
    fn get_task_type_guidance(&self, task: &str) -> String {
        let task_lower = task.to_lowercase();

        // 编译类任务
        if task_lower.contains("编译")
            || task_lower.contains("build")
            || task_lower.contains("make")
        {
            return r#"**识别为：编译/构建任务**

用户意图：将源代码编译为可执行文件或库

**必须分解的完整步骤**：
1. **确认源码存在** - 检查压缩包或源码目录
2. **解压源码**（如果是压缩包）- 使用 archive_unpack
3. **查找和阅读文档** - 查找 README、INSTALL、BUILDING、docs/ 等文档，了解构建要求和步骤
4. **检查构建系统** - 查看 Makefile/CMakeLists.txt/configure 等
5. **检查编译工具** - 确认 gcc/g++/make/cmake 等存在
6. **执行编译** - 运行 make/cmake 等构建命令
7. **验证产物** - 用 `read_dir` 等检查构建输出目录中是否出现可执行文件/预期目标
8. **运行并核对** - 用 **`run_executable`** 等工作区内运行能力执行产物、核对退出码与（如有）标准输出；用户若要求「能跑起来」或 Hello World/演示，**本步与前面步骤同等重要**，子目标**不得**停在仅编译通过

**CMake 编写要点**（模型生成 `CMakeLists.txt` 时）：
- 单文件示例程序用 **`add_executable(目标 main.cpp)`**，避免根目录 **`file(GLOB_RECURSE "*.cpp")`** 把 `build/CMakeFiles/**/CMake*CompilerId.*` 编进目标引发链接错误

**重要**：
- 不要只分解"检查"步骤，必须包含完整的编译与（若适用）**运行**流程！
- **务必先阅读文档** - 很多项目有特定的构建要求和依赖，文档中会说明正确的构建步骤
"#
            .to_string();
        }

        // 代码修改类任务
        if task_lower.contains("修改") || task_lower.contains("修复") || task_lower.contains("fix")
        {
            return r#"**识别为：代码修改/修复任务**

用户意图：修改代码文件以修复问题或实现功能

**必须分解的完整步骤**：
1. **定位目标文件** - 找到需要修改的文件
2. **读取当前内容** - 使用 read_file 查看文件
3. **执行修改** - 使用 search_replace 或 modify_file
4. **验证修改** - 读取文件确认修改成功
5. **测试**（如需要）- 运行相关测试验证修复

**重要**：不要只分解"查找"步骤，必须包含实际的修改操作！
"#
            .to_string();
        }

        // 分析/调查类任务
        if task_lower.contains("分析") || task_lower.contains("查看") || task_lower.contains("调查")
        {
            return r#"**识别为：分析/调查任务**

用户意图：收集信息、分析问题或查看状态

**分解要点**：
- 明确需要收集哪些信息
- 确定信息来源（日志文件、配置文件、目录结构等）
- 如果需要多步骤分析，确保步骤之间有逻辑关联

**重要**：分析任务应该产出明确的结论或报告！
"#
            .to_string();
        }

        // 默认指导
        r#"**通用任务**

请确保：
1. 完整理解用户意图 - 不要只分解验证/检查步骤
2. 子目标应该覆盖任务的完整生命周期
3. 如果任务涉及多个阶段（准备→执行→验证），确保每个阶段都有对应的子目标
"#
        .to_string()
    }

    /// 分解、重试、重规划、反思、失败处理等阶段共用的**工具/JSON 固定规范**（写入 Manager 提示，保证子目标与 Executor 调用工具一致）。
    fn manager_tool_invariants() -> &'static str {
        r#"- 分配工具时，确保参数与子目标、工具 `parameters` 一致
- **`create_file` 是向工作区新建普通文件的正确方式**；禁止用 `echo`/`cat`/`tee` 经 `run_command` 建文件
- `path` 优先**相对工作区根**（如 `main.cpp`）；**勿**在子目标或工具参数里造深层误路径（如无关子目录下的 `main.cpp`）
- `create_file` 仅当目标路径**尚不存在**时成功；已存在时须用 `modify_file`、`search_replace`、`append_file` 等，**禁止**对同一路径重复 `create_file`
- `run_command` 须分别传 `command` 与 `args`；`args` 在 JSON 中必须是**字符串**数组。每一项**必须**用双引号包起来（如列表标志为 `\"-la\"` 的数组元素，不得写成无引号 token），否则易触发「参数解析错误」或 `invalid number`
- 查可执行/依赖是否在 PATH 时用 **`"command": "which"` + `"args": ["cmake"]`** 等；**禁止**写成 `\"command\": \"which cmake\"` 单字段（会把整串当程序名而失败）
- 简单 CMake 项目用 **`add_executable(… main.cpp)`** 等**显式列出源文件**；勿对**空** `file(GLOB …)` 结果生成目标（会无源可链）；**勿**用未排除 `build/` 的 **`GLOB_RECURSE`** 收集 `*.cpp`（会把 `CMakeFiles/` 下探测源链进来导致重复 `main`）
- 工作区内的**可执行/构建产物**的「运行（执行）」优先用 **`run_executable` + 相对工作区根路径**；白名单**系统**命令用 `run_command`；以工具说明与 `config` 中分工为准
- 子目标若属于「运行可执行体 / 验证程序输出」：`required_tools` **必须包含** `run_executable`，并以 `run_executable` 为主执行；`run_command` 仅作补充诊断，不得替代主验证
- 子目标若属于「编译构建」：描述与步骤中**禁止**包含运行可执行体动作；运行与输出核对应拆到独立后续子目标
- 须**从源码到可跑通、输出可核对**的完整类任务，子目标**必须**含**运行产物并验证**的一步；**不得**在「只编译/只生成文件」时视为整任务完成
- `read_dir` 路径为不含 `..` 的相对路径
- `create_file` 的 `content` 为 JSON 字符串，须按规范对换行、引号等转义"#
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
    fn parse_output(
        &self,
        content: &str,
        finish_reason: Option<&str>,
    ) -> Result<ManagerOutput, ManagerError> {
        let json_str = extract_json(content).ok_or_else(|| {
            let diag = extract_json_diagnostic(content);
            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: failed to extract JSON from response: finish_reason={:?} head={:?} tail={:?} depth={} in_string={}",
                finish_reason,
                truncate_for_log(content, 200),
                truncate_for_log(diag.tail.as_str(), 200),
                diag.depth,
                diag.in_string,
            );
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
            #[serde(default)]
            consumes_from_dependencies: Option<Vec<super::task::DependencyContractEntry>>,
            required_tools: Option<Vec<String>>,
            #[serde(default)]
            goal_type: Option<super::task::GoalType>,
            #[serde(default)]
            build_requirements: Option<super::task::BuildRequirements>,
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
            let mut g = SubGoal {
                goal_id: sg.goal_id,
                description: sg.description,
                priority: sg.priority.unwrap_or(0),
                depends_on: sg.depends_on.unwrap_or_default(),
                consumes_from_dependencies: sg.consumes_from_dependencies.unwrap_or_default(),
                required_tools: sg.required_tools.unwrap_or_default(),
                goal_type: sg.goal_type.unwrap_or_default(),
                build_requirements: sg.build_requirements.unwrap_or_default(),
                acceptance: None,
                max_retries: None,
            };
            super::subgoal_context::normalize_subgoal_io_contracts(&mut g);
            sub_goals.push(g);
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

    /// 分解/重规划输出：首次解析失败时，最多一次「仅修 JSON」补调用（不改语义）。
    async fn parse_output_with_one_json_repair(
        &self,
        content: &str,
        finish_reason: Option<&str>,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<ManagerOutput, ManagerError> {
        const MANAGER_JSON_REPAIR_LLM: bool = true;
        match self.parse_output(content, finish_reason) {
            Ok(out) => Ok(out),
            Err(parse_err) if MANAGER_JSON_REPAIR_LLM => {
                log::warn!(
                    target: "crabmate",
                    "[HIERARCHICAL] Manager: plan parse failed, attempting one-shot JSON repair LLM: {}",
                    parse_err
                );
                let json_fragment = extract_json_candidate_for_repair(content);
                let repair_user = Self::build_manager_json_repair_user_prompt(
                    json_fragment.as_str(),
                    &parse_err.to_string(),
                );
                let mut messages = vec![Message::user_only(json_fragment)];
                messages.push(Message::user_only(repair_user));
                let mut request = no_tools_chat_request_for_hierarchical_manager(
                    cfg,
                    &messages,
                    Some(Self::MANAGER_JSON_REPAIR_TEMPERATURE),
                    None,
                    LlmSeedOverride::FromConfig,
                );
                Self::force_manager_structured_json_mode(&mut request);
                let response = complete_chat_retrying(
                    &CompleteChatRetryingParams::new(
                        llm_backend,
                        client,
                        api_key,
                        cfg,
                        LlmRetryingTransportOpts::headless_no_stream(),
                        None,
                        None,
                    ),
                    &request,
                )
                .await
                .map_err(|e| ManagerError::LlmError(e.to_string()))?;
                let fixed = message_content_as_str(&response.0.content).unwrap_or_default();
                log::debug!(
                    target: "crabmate",
                    "[HIERARCHICAL] Manager: plan JSON repair response preview: {}",
                    truncate_for_log(fixed, 500)
                );
                self.parse_output(fixed, None)
            }
            Err(e) => Err(e),
        }
    }

    /// 获取执行策略
    pub fn execution_strategy(&self) -> ExecutionStrategy {
        self.config.execution_strategy
    }

    /// 检查是否是编译类任务
    fn is_compile_task(&self, task: &str) -> bool {
        let task_lower = task.to_lowercase();
        task_lower.contains("编译")
            || task_lower.contains("build")
            || task_lower.contains("make")
            || task_lower.contains("cmake")
    }

    /// 从任务描述中提取可执行文件名
    fn extract_executable_name(&self, task: &str) -> Option<String> {
        let task_lower = task.to_lowercase();

        // 尝试匹配 "编译 xxx" 或 "build xxx" 模式
        let patterns = [
            r"编译\s+(\w+)",
            r"build\s+(\w+)",
            r"make\s+(\w+)",
            r"编译\s+(\w+)\s+源码",
            r"编译\s+(\w+)\s+源代码",
        ];

        for pattern in &patterns {
            if let Ok(re) = regex::Regex::new(pattern)
                && let Some(cap) = re.captures(&task_lower)
                && let Some(name) = cap.get(1)
            {
                return Some(name.as_str().to_lowercase());
            }
        }

        // 尝试从源码包名称提取（如 hpcg-HPCG-release-3-1-0.tar.gz -> hpcg）
        if let Ok(re) = regex::Regex::new(r"(\w+)[-_].*\.(tar\.gz|tgz|zip)")
            && let Some(cap) = re.captures(&task_lower)
            && let Some(name) = cap.get(1)
        {
            return Some(name.as_str().to_lowercase());
        }

        None
    }

    /// 检查可执行文件是否已存在
    fn check_existing_executable(&self, task: &str) -> Option<std::path::PathBuf> {
        // 1. 首先检查会话状态
        if let Some(name) = self.extract_executable_name(task) {
            if let Some(path) = self.is_executable_built(&name) {
                return Some(path);
            }

            // 2. 检查常见的可执行文件路径
            let common_paths = [
                format!("{}/bin/{}", name, name),
                format!("{}/bin/x{}", name, name),
                format!("{}/{}", name, name),
                format!("bin/{}", name),
                format!("build/{}", name),
                name.clone(),
            ];

            for path_str in &common_paths {
                let path = std::path::Path::new(path_str);
                if path.exists() && path.is_file() {
                    // 检查是否是可执行文件（Unix 系统）
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(metadata) = path.metadata() {
                            let permissions = metadata.permissions();
                            if permissions.mode() & 0o111 != 0 {
                                return Some(path.to_path_buf());
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        return Some(path.to_path_buf());
                    }
                }
            }
        }

        None
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
        let mut request = no_tools_chat_request_for_hierarchical_manager(
            cfg,
            &messages,
            Some(Self::REFLECTION_PRIMARY_TEMPERATURE),
            None,
            LlmSeedOverride::FromConfig,
        );
        Self::force_manager_structured_json_mode(&mut request);

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

        // 解析更新的子目标；失败时最多一次「仅修 JSON」的补调用，提高鲁棒性
        const REFLECT_JSON_REPAIR_LLM: bool = true;
        match self.parse_reflection_output(failed_goal, content, working_dir, false) {
            Ok(goal) => Ok(goal),
            Err(parse_err) if REFLECT_JSON_REPAIR_LLM => {
                log::warn!(
                    target: "crabmate",
                    "[HIERARCHICAL] Manager: reflection parse failed, attempting JSON repair LLM: {}",
                    parse_err
                );
                let json_fragment = extract_json_candidate_for_repair(content);
                let repair_user = Self::build_reflection_json_repair_user_prompt(
                    json_fragment.as_str(),
                    &parse_err.to_string(),
                );
                let mut messages = vec![Message::user_only(json_fragment)];
                messages.push(Message::user_only(repair_user));
                let mut request = no_tools_chat_request_for_hierarchical_manager(
                    cfg,
                    &messages,
                    Some(Self::REFLECTION_JSON_REPAIR_TEMPERATURE),
                    None,
                    LlmSeedOverride::FromConfig,
                );
                Self::force_manager_structured_json_mode(&mut request);
                let response = complete_chat_retrying(&params, &request)
                    .await
                    .map_err(|e| ManagerError::LlmError(e.to_string()))?;
                let fixed = message_content_as_str(&response.0.content).unwrap_or_default();
                log::debug!(
                    target: "crabmate",
                    "[HIERARCHICAL] Manager: JSON repair response preview: {}",
                    truncate_for_log(fixed, 500)
                );
                self.parse_reflection_output(failed_goal, fixed, working_dir, true)
            }
            Err(e) => Err(e),
        }
    }

    /// 分层反思：为降低发散，使用略低于主配置的温度
    const REFLECTION_PRIMARY_TEMPERATURE: f32 = 0.25;
    /// 仅修 JSON 的补调用：更低温以约束格式
    const REFLECTION_JSON_REPAIR_TEMPERATURE: f32 = 0.0;
    /// 分解/重规划 JSON 修复补调用温度（仅修格式/枚举，不改计划语义）。
    const MANAGER_JSON_REPAIR_TEMPERATURE: f32 = 0.0;

    /// 构建反思「结构化执行摘要」供模型与日志共用
    fn build_reflection_structured_block(
        failed_goal: &SubGoal,
        verification_failure: &str,
        execution_result: &TaskResult,
    ) -> String {
        let (task_status, fail_reason) = match &execution_result.status {
            TaskStatus::Failed { reason } => ("Failed", reason.as_str()),
            TaskStatus::Completed => ("Completed", ""),
            TaskStatus::Skipped { reason } => ("Skipped", reason.as_str()),
            TaskStatus::NeedsDecomposition { reason, .. } => {
                ("NeedsDecomposition", reason.as_str())
            }
            TaskStatus::Pending | TaskStatus::InProgress => {
                ("Other", "goal not in terminal state in reflection context")
            }
        };
        let out_preview = truncate_for_log(execution_result.output.as_deref().unwrap_or(""), 500);
        let err_preview = truncate_for_log(execution_result.error.as_deref().unwrap_or(""), 500);
        let inv = execution_result.tools_invoked.to_vec().join(", ");
        let inv_display = if inv.is_empty() {
            "(无)".to_string()
        } else {
            inv
        };
        format!(
            r#"## 结构化执行摘要（机器可读）
- **execution.task_status**（子目标内执行状态，非最终验收）: `{}`
- **execution.task_id**: `{}`
- **execution.status_detail**: `{}`
- **execution.verification_failure**（与 GoalVerifier 的判定一致）: `{}`
- **execution.tools_invoked**（时间顺序，工具名）: {}
- **execution.output_preview**:
```
{}
```
- **execution.error_preview**:
```
{}
```

## 原失败的子目标（同上文 goal_id 一致）当前字段速览
- `goal_id`: `{}`
- `current_acceptance`: {:?}
"#,
            task_status,
            execution_result.task_id,
            fail_reason,
            verification_failure,
            inv_display,
            out_preview,
            err_preview,
            failed_goal.goal_id,
            failed_goal.acceptance,
        )
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
        let structured = Self::build_reflection_structured_block(
            failed_goal,
            verification_failure,
            execution_result,
        );

        format!(
            r#"## 任务
你是一个任务修复专家。子目标执行后验证失败，需要分析原因并生成修复策略。

{structured}

## 失败的子目标
- goal_id: {}
- description: {}
- goal_type: {:?}
- depends_on: {:?}
- consumes_from_dependencies（当前）: {:?}
- build_requirements（当前）: {:?}
- required_tools: {:?}

## 验证失败原因（与结构化摘要中 verification_failure 相同）
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

## 子目标/工具调用的固定规范
{}

## 验收与验证命令的约束
- `acceptance.expect_file_exists` 须为**工作区根目录下的相对路径**（可含 `/`，**禁止**以 `/` 开头、**禁止** `..` 段）。
- `acceptance.expect_command_success`（若填写）为**无 shell 拼接**的单一验证命令，仅允许**直接执行**的 argv（见下）；**不要**用 `;`、`|`、`&`、`$()`、反引号 或**重定向**；**禁止** `..` 与**绝对路径**实参。推荐示例：`test -f path/to/file`、`./build/foo --help`（在仓库根为 cwd 下可解析）、`cargo test -q --`。
- 仅需要文件或输出片段验证时，可**省略** `expect_command_success`（用 `expect_file_exists` 与/或 `expect_output_contains`）。

## 输出格式
**必须输出一个 JSON 对象**（可放在 markdown 代码块中），**顶层键**须包含 `analysis`、`fix_strategy`、`updated_goal`。**除 JSON 外不要**输出其他解释文字。JSON 须符合结构：
```json
{{
  "analysis": "失败原因分析（简要说明为什么验证失败）",
    "fix_strategy": "修复策略（简要说明如何修复）",
    "updated_goal": {{
        "goal_id": "{}",
        "description": "更新后的子目标描述（更具体、包含修复步骤；I/O 与 `{{ref:...}}` 引用）",
        "priority": {},
        "depends_on": {:?},
        "consumes_from_dependencies": [{{"from_goal_id": "须出现在 depends_on 中", "only_kinds": null}} 或 {{"from_goal_id": "g1", "only_kinds": ["all"]}} 或 {{"from_goal_id": "g1", "only_kinds": ["executable"]}}],
        "build_requirements": {{"needs_artifacts": [], "produces_artifacts": []}} 可省略,
        "required_tools": {:?},
        "goal_type": "{:?}",
        "acceptance": {{
            "expect_file_exists": ["相对路径1", "相对路径2"],
            "expect_command_success": "仅在有把握时使用；否则省略为 null 或空字符串并省略此键",
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
3. 若失败与**未消费对的前序路径**、**`{{ref:...}}` 未填**、或 **only_kinds 过严** 有关，必须在 `updated_goal` 中**显式**给出 `consumes_from_dependencies` 与/或 调整 `depends_on`；`from_goal_id` 必须出现在 `depends_on` 中
4. 添加或完善 `acceptance` 条件，确保下次能正确验证
5. `max_retries` 控制验证失败后的重试次数
"#,
            failed_goal.goal_id,
            failed_goal.description,
            failed_goal.goal_type,
            failed_goal.depends_on,
            failed_goal.consumes_from_dependencies,
            failed_goal.build_requirements,
            failed_goal.required_tools,
            verification_failure,
            execution_output,
            execution_error,
            workspace_context,
            artifacts_summary,
            tools_description,
            Self::manager_tool_invariants(),
            failed_goal.goal_id,
            failed_goal.priority,
            failed_goal.depends_on,
            failed_goal.required_tools,
            failed_goal.goal_type,
            structured = structured,
        )
    }

    /// 第二次调用：在首轮模型输出上仅提取/修复符合模式的 JSON
    fn build_reflection_json_repair_user_prompt(json_fragment: &str, parse_error: &str) -> String {
        let err_preview = truncate_for_log(parse_error, 500);
        let a = truncate_to_char_boundary(json_fragment, 12_000);
        format!(
            r#"上一次你在尝试输出反思 JSON 时，解析器报错如下（**不要**在输出中照抄本说明文字）:
{err_preview}

下面是你上一次回答中的 JSON 片段。请**只**输出**一个**合法 JSON 对象，**顶层键**必须包含:
`analysis`（string）、`fix_strategy`（string）、`updated_goal`（object，与首轮 schema 相同）。
不要 markdown 代码围栏以外的文字。若上一条在代码块中已有 JSON，请**修正**其中的引号/逗号/花括号 使其可被标准 JSON 解析。

---BEGIN_ASSISTANT---
{a}
---END---"#
        )
    }

    /// 分解/重规划第二次调用：仅提取/修复 `sub_goals + execution_strategy` JSON。
    fn build_manager_json_repair_user_prompt(json_fragment: &str, parse_error: &str) -> String {
        let err_preview = truncate_for_log(parse_error, 500);
        let a = truncate_to_char_boundary(json_fragment, 12_000);
        format!(
            r#"上一次你在输出任务分解 JSON 时，解析器报错如下（**不要**在输出中照抄本说明文字）:
{err_preview}

下面是你上一次回答中的 JSON 片段。请**只**输出**一个**合法 JSON 对象，顶层键必须是：
`sub_goals`（array）与 `execution_strategy`（string）。
不要输出 markdown 代码围栏，不要输出任何解释。

要求：
1. 只修 JSON 结构与字段值合法性（引号、逗号、括号、枚举值），不改变原计划语义；
2. `build_requirements.needs_artifacts / produces_artifacts` 仅允许：
`SourceFile` / `ObjectFile` / `Executable` / `StaticLibrary` / `DynamicLibrary` / `BuildLog`；
   若出现 `File`、`BuildFile` 等别名，请改为 `SourceFile`。
3. 严格满足下方 Schema（required / enum / additionalProperties）：
```json
{}
```

---BEGIN_ASSISTANT---
{a}
---END---"#,
            Self::manager_output_schema_contract()
        )
    }

    /// 解析反思输出
    fn parse_reflection_output(
        &self,
        original_goal: &SubGoal,
        content: &str,
        workspace_root: &std::path::Path,
        is_repair_pass: bool,
    ) -> Result<SubGoal, ManagerError> {
        let label = if is_repair_pass {
            "JSON repair"
        } else {
            "reflection"
        };
        let json_str = extract_json(content).ok_or_else(|| {
            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: failed to extract JSON from {label} response"
            );
            ManagerError::ParseError(format!("Failed to extract JSON from {label} response"))
        })?;

        // 尝试修复常见的 JSON 格式错误
        let fixed_json = fix_common_json_errors(json_str);
        let json_to_parse = if fixed_json != json_str {
            log::debug!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: applied JSON format fixes"
            );
            fixed_json.as_str()
        } else {
            json_str
        };

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
            #[serde(default)]
            consumes_from_dependencies: Option<Vec<super::task::DependencyContractEntry>>,
            required_tools: Option<Vec<String>>,
            #[serde(default)]
            goal_type: Option<super::task::GoalType>,
            #[serde(default)]
            build_requirements: Option<super::task::BuildRequirements>,
            #[serde(default)]
            acceptance: Option<super::task::GoalAcceptance>,
            #[serde(default)]
            max_retries: Option<usize>,
        }

        let mut parsed: ReflectionOutput = serde_json::from_str(json_to_parse).map_err(|e| {
            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: JSON parse error in {label}: {}. Content preview: {}",
                e,
                truncate_for_log(json_to_parse, 200)
            );
            ManagerError::ParseError(e.to_string())
        })?;

        if parsed.updated_goal.goal_id != original_goal.goal_id {
            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: reflection changed goal_id from {} to {}, using original",
                original_goal.goal_id,
                parsed.updated_goal.goal_id
            );
        }

        let (acceptance, dropped_cmd) =
            sanitize_reflection_acceptance(parsed.updated_goal.acceptance.take(), workspace_root);
        if dropped_cmd {
            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: dropped unsafe expect_command_success from reflection JSON"
            );
        }
        let mut updated_goal = SubGoal {
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
            consumes_from_dependencies: parsed
                .updated_goal
                .consumes_from_dependencies
                .unwrap_or_else(|| original_goal.consumes_from_dependencies.clone()),
            required_tools: parsed
                .updated_goal
                .required_tools
                .unwrap_or_else(|| original_goal.required_tools.clone()),
            goal_type: parsed
                .updated_goal
                .goal_type
                .unwrap_or(original_goal.goal_type.clone()),
            build_requirements: parsed
                .updated_goal
                .build_requirements
                .unwrap_or_else(|| original_goal.build_requirements.clone()),
            acceptance,
            max_retries: parsed
                .updated_goal
                .max_retries
                .or(original_goal.max_retries),
        };
        super::subgoal_context::normalize_subgoal_io_contracts(&mut updated_goal);

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
        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: reflection produced updated goal with acceptance={}",
            updated_goal.acceptance.is_some()
        );

        Ok(updated_goal)
    }
}

/// 在 utf-8 下按**字符**数截断到上限（用于打包进二轮 JSON 修复的 user 消息，避免超上下文）。
fn truncate_to_char_boundary(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut n = 0;
    for (i, c) in s.char_indices() {
        n += 1;
        if n == max_chars {
            let end = i + c.len_utf8();
            return &s[..end];
        }
    }
    s
}

/// 反思产出的 `acceptance` 与 `GoalVerifier::run_verify_command`（无 shell 的 argv）对齐：去路径穿越、
/// 去 shell 元字符；`expect_file_exists` 规范为可拼进工作区根的相对路径。
fn sanitize_reflection_acceptance(
    acc: Option<super::task::GoalAcceptance>,
    _workspace_root: &std::path::Path,
) -> (Option<super::task::GoalAcceptance>, bool) {
    let mut dropped_cmd = false;
    let Some(mut a) = acc else {
        return (None, false);
    };

    let mut paths = Vec::new();
    for p in std::mem::take(&mut a.expect_file_exists) {
        if p.trim().is_empty() {
            continue;
        }
        if is_unsafe_path_segment(&p) {
            log::debug!(
                target: "crabmate",
                "[HIERARCHICAL] Manager: skipping unsafe expect_file_exists: {}",
                truncate_for_log(&p, 80)
            );
            continue;
        }
        // 规范化为相对、POSIX 式斜杠，便于与 GoalVerifier 拼接
        let cleaned = p.trim().trim_start_matches('/').to_string();
        if !cleaned.is_empty() {
            paths.push(cleaned);
        }
    }
    a.expect_file_exists = paths;

    if let Some(ref cmd) = a.expect_command_success
        && is_unsafe_verify_command(cmd)
    {
        log::warn!(
            target: "crabmate",
            "[HIERARCHICAL] Manager: expect_command_success rejected, clearing: {}",
            truncate_for_log(cmd, 200)
        );
        a.expect_command_success = None;
        dropped_cmd = true;
    }

    if a.expect_file_exists.is_empty()
        && a.expect_output_contains.is_empty()
        && a.expect_command_success.is_none()
        && a.expect_exit_code.is_none()
    {
        (None, dropped_cmd)
    } else {
        (Some(a), dropped_cmd)
    }
}

fn is_unsafe_path_segment(s: &str) -> bool {
    use std::path::{Component, Path};

    if s.starts_with('/') {
        return true;
    }
    if s.contains("..") {
        return true;
    }
    let p = Path::new(s);
    for c in p.components() {
        if c == Component::ParentDir {
            return true;
        }
        if let Component::RootDir = c {
            return true;
        }
    }
    // 疑似 `C:...` / `C:\` 等 Windows 前缀（本验收为工作区相对路径）
    s.len() >= 2 && s.as_bytes()[0].is_ascii_alphabetic() && s.as_bytes()[1] == b':'
}

/// 与 `run_verify_command`（`split_whitespace` + 无 shell）一致；禁止 shell 注入与 `..` 实参
fn is_unsafe_verify_command(cmd: &str) -> bool {
    if cmd.is_empty() {
        return true;
    }
    let t = cmd.trim();
    for ch in [
        ';', '&', '|', '>', '<', '\n', '\r', '\t', '`', '$', '(', ')', '*', '?', '[', ']',
    ] {
        if t.contains(ch) {
            return true;
        }
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    if parts.len() >= 2
        && parts[1] == "-c"
        && matches!(
            parts[0].to_lowercase().as_str(),
            "sh" | "bash" | "dash" | "zsh" | "cmd" | "ksh" | "fish" | "pwsh"
        )
    {
        return true;
    }
    for w in t.split_whitespace() {
        if w.contains("..") {
            return true;
        }
        if w == "/" || w.starts_with('/') {
            return true;
        }
        if w == "\\" {
            return true;
        }
        if w.contains(':')
            && w.len() == 2
            && let Some(first) = w.chars().next()
            && w.ends_with(':')
            && first.is_ascii_alphabetic()
        {
            return true;
        }
    }
    // 过长的单条可能是不慎粘贴的 shell 串
    t.chars().count() > 512
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

/// 从响应中提取最外层 JSON 对象切片（跳过前文噪音）。
///
/// 须在 **双引号字符串外**统计 `{}`，否则子目标 `description` 等字段中的 `{` / `}` 会破坏朴素括号计数，
/// 导致永远找不到匹配的闭合括号（表现为 `Failed to extract JSON`）。
fn extract_json(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escape = false;
    for (rel_byte, c) in content[start..].char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match c {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = start + rel_byte + c.len_utf8();
                    return Some(&content[start..end]);
                }
            }
            _ => {}
        }
    }
    None
}

/// 为 JSON 修复补调用提取“最像 JSON”的候选片段，尽量降低噪声。
fn extract_json_candidate_for_repair(content: &str) -> String {
    if let Some(s) = extract_json(content) {
        return s.to_string();
    }
    if let Some(start) = content.find('{') {
        return truncate_to_char_boundary(&content[start..], 12_000).to_string();
    }
    truncate_to_char_boundary(content, 12_000).to_string()
}

#[derive(Debug)]
struct ExtractJsonDiagnostic {
    depth: u32,
    in_string: bool,
    tail: String,
}

/// JSON 提取失败时的快速诊断：
/// - `depth > 0`：大概率为输出被截断（缺失闭合 `}`）；
/// - `in_string = true`：字符串可能未闭合；
/// - `tail`：末尾片段，便于定位中断点。
fn extract_json_diagnostic(content: &str) -> ExtractJsonDiagnostic {
    let start = content.find('{').unwrap_or(0);
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escape = false;
    for c in content[start..].chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match c {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let tail: String = content
        .chars()
        .rev()
        .take(200)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    ExtractJsonDiagnostic {
        depth,
        in_string,
        tail,
    }
}

/// 尝试修复常见的 LLM JSON 格式错误
fn fix_common_json_errors(content: &str) -> String {
    let mut fixed = content.to_string();

    // 修复 Perl/Ruby 风格的 => 语法为标准的 JSON :
    // 但需要注意不要破坏 URL 中的 => 或合法的字符串内容
    // 使用正则替换：在非字符串上下文中将 => 替换为 :
    let re = regex::Regex::new(r"(\w+)\s*=>\s*").unwrap();
    fixed = re.replace_all(&fixed, r#"$1": "#).to_string();

    // 修复单引号为双引号（如果整个 JSON 使用单引号）
    // 注意：这可能会破坏包含单引号的字符串，需要谨慎
    // 只在检测到 JSON 以单引号开始时替换
    if fixed.trim().starts_with("'") {
        fixed = fixed.replace("'", "\"");
    }

    fixed
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

    #[test]
    fn test_extract_json_diagnostic_reports_unclosed_object() {
        let d = extract_json_diagnostic(r#"{"sub_goals":[{"goal_id":"g1""#);
        assert!(d.depth > 0);
        assert!(!d.tail.is_empty());
    }

    #[test]
    fn extract_json_candidate_prefers_json_slice() {
        let raw = "前文噪音\n{\"sub_goals\":[]}\n后文";
        let out = extract_json_candidate_for_repair(raw);
        assert_eq!(out, "{\"sub_goals\":[]}");
    }

    #[test]
    fn test_extract_json_braces_inside_string_values() {
        let content = r#"{"sub_goals":[{"goal_id":"g1","description":"查看 {src} 与 } 符号","priority":1,"depends_on":[],"required_tools":[]}],"execution_strategy":"sequential"}"#;
        let json = extract_json(content).unwrap();
        assert!(json.contains("sub_goals"));
        assert!(json.contains("{src}"));
        let v: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        assert!(v.get("sub_goals").is_some());
    }

    #[test]
    fn test_sanitize_reflection_acceptance_paths_and_cmd() {
        use super::super::task::GoalAcceptance;

        let acc = Some(GoalAcceptance {
            expect_file_exists: vec!["/etc/passwd".to_string(), "ok/rel".to_string()],
            expect_command_success: Some("rm -rf /".to_string()),
            expect_output_contains: vec![],
            expect_exit_code: None,
        });
        let (out, dropped) =
            super::sanitize_reflection_acceptance(acc, std::path::Path::new("/tmp/ws"));
        assert!(dropped);
        let g = out.expect("some");
        assert_eq!(g.expect_file_exists, vec!["ok/rel".to_string()]);
        assert!(g.expect_command_success.is_none());
    }

    #[test]
    fn test_is_unsafe_verify_command_allows_simple_argv() {
        assert!(!super::is_unsafe_verify_command("test -f build/foo"));
        assert!(super::is_unsafe_verify_command("sh -c echo"));
    }
}
