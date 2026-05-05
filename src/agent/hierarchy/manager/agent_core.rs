//! 分解、重规划入口、失败子目标处理（LLM 编排）。

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{
    CompleteChatRetryingParams, LlmRetryingTransportOpts, complete_chat_retrying,
    no_tools_chat_request_for_hierarchical_manager,
};
use crate::types::{LlmSeedOverride, Message, message_content_as_str};

use super::super::session_state::SessionStateManager;
use super::super::task::{ExecutionStrategy, SubGoal};
use super::manager_tail::{ManagerOutput, truncate_for_log, truncate_task};
use super::types::{ManagerAgent, ManagerConfig, ManagerDecision, ManagerError, ManagerLlmContext};

impl ManagerAgent {
    /// 初次分解与重规划共用的「分解硬性规则」第 1～10 条（单源维护，避免两处 prompt 漂移）。
    pub(super) const DECOMPOSITION_RULES_1_TO_10: &'static str = r#"1. 子目标必须**单一职责**，禁止跨步执行。
2. 每个子目标只允许执行其描述中的动作，禁止提前执行后续步骤。
3. 产物命名必须全程一致；一旦确定名称（如可执行文件名），后续不得改名或混用。
4. `depends_on` 必须准确，后续步骤不得绕过依赖。
5. 每个子目标必须给出可验证的 I/O 与验收目标，且与本步职责严格匹配。
6. 若某步是“创建文件”，描述中只允许文件创建/内容核验，不得包含配置、编译或运行动作。
7. 若某步是“配置构建”，描述中只允许配置动作，不得包含编译或运行动作。
8. 若某步是“编译”，描述中只允许编译与产物核验，不得运行程序。
9. 若某步是“运行验证”，描述中只允许运行与输出核验。
10. 若任务涉及 C++ + CMake，默认采用稳定链路：检查目录 → 写 `main.cpp` → 写 `CMakeLists.txt` → `cmake -S . -B build` → `cmake --build build` → 运行产物；且可执行文件名需在 `CMakeLists.txt` 与后续子目标中保持一致（与 `add_executable` 目标名一致，勿随意改名）。"#;

    /// Manager 分解/重规划输出的强约束 schema（提示词用）。
    pub(super) fn manager_output_schema_contract() -> &'static str {
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
    pub(super) fn force_manager_structured_json_mode(req: &mut crate::types::ChatRequest) {
        req.vendor.thinking = None;
        req.vendor.reasoning_split = None;
        req.vendor.reasoning_effort = None;
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
    pub(super) fn should_execute_task(&self, task: &str) -> bool {
        if let Some(ref manager) = self.session_manager {
            manager.should_execute_task(task)
        } else {
            true // 没有会话管理器时，默认执行
        }
    }

    /// 检查可执行文件是否已构建
    pub(super) fn is_executable_built(&self, name: &str) -> Option<std::path::PathBuf> {
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

        let mut messages = vec![Message::user_only(&prompt)];
        crate::agent::context_window::prepare_messages_for_hierarchical_llm_sync(
            &mut messages,
            cfg,
        );
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
    pub(super) fn decompose_fallback(&self, task: &str) -> ManagerOutput {
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
    pub async fn replan_with_artifacts(
        &self,
        original_task: &str,
        llm: ManagerLlmContext<'_>,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        previous_results: &[super::super::task::TaskResult],
        previous_artifacts: &[super::super::task::Artifact],
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

        let mut messages = vec![Message::user_only(&prompt)];
        crate::agent::context_window::prepare_messages_for_hierarchical_llm_sync(
            &mut messages,
            llm.cfg,
        );
        let mut request = no_tools_chat_request_for_hierarchical_manager(
            llm.cfg,
            &messages,
            None,
            None,
            LlmSeedOverride::FromConfig,
        );
        Self::force_manager_structured_json_mode(&mut request);

        match complete_chat_retrying(
            &CompleteChatRetryingParams::new(
                llm.llm_backend,
                llm.client,
                llm.api_key,
                llm.cfg,
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
                    llm.cfg,
                    llm.llm_backend,
                    llm.client,
                    llm.api_key,
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
    pub async fn handle_failed_goal(
        &self,
        failed_goal: &SubGoal,
        error_message: &str,
        llm: ManagerLlmContext<'_>,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        previous_artifacts: &[super::super::task::Artifact],
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

        let mut messages = vec![Message::user_only(&prompt)];
        crate::agent::context_window::prepare_messages_for_hierarchical_llm_sync(
            &mut messages,
            llm.cfg,
        );
        let mut request = no_tools_chat_request_for_hierarchical_manager(
            llm.cfg,
            &messages,
            None,
            None,
            LlmSeedOverride::FromConfig,
        );
        Self::force_manager_structured_json_mode(&mut request);

        let params = CompleteChatRetryingParams::new(
            llm.llm_backend,
            llm.client,
            llm.api_key,
            llm.cfg,
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
}
