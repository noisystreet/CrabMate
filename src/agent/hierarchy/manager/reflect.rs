//! 验证失败后的反思与 `updated_goal` 解析。

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{
    CompleteChatRetryingParams, LlmRetryingTransportOpts, complete_chat_retrying,
    no_tools_chat_request_for_hierarchical_manager,
};
use crate::types::{LlmSeedOverride, Message, message_content_as_str};

use super::super::task::{Artifact, SubGoal, TaskResult, TaskStatus};
use super::manager_tail::{
    fix_common_json_errors, sanitize_reflection_acceptance, truncate_for_log,
};
use super::types::{ManagerAgent, ManagerError};

/// 反思并重规划子目标所需的 LLM 与任务上下文（缩短 [`ManagerAgent::reflect_and_replan`] 形参列表）。
pub struct ReflectAndReplanContext<'a> {
    pub failed_goal: &'a SubGoal,
    pub verification_failure: &'a str,
    pub execution_result: &'a TaskResult,
    pub cfg: &'a AgentConfig,
    pub llm_backend: &'a dyn ChatCompletionsBackend,
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub working_dir: &'a std::path::Path,
    pub tools_defs: &'a [crate::types::Tool],
    pub artifacts: &'a [Artifact],
}

impl ManagerAgent {
    /// 反思验证失败并重新规划子目标
    ///
    /// 当验证失败时，分析失败原因并生成修复策略
    pub async fn reflect_and_replan(
        &self,
        ctx: ReflectAndReplanContext<'_>,
    ) -> Result<SubGoal, ManagerError> {
        let ReflectAndReplanContext {
            failed_goal,
            verification_failure,
            execution_result,
            cfg,
            llm_backend,
            client,
            api_key,
            working_dir,
            tools_defs,
            artifacts,
        } = ctx;
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

        let mut messages = vec![Message::user_only(&prompt)];
        crate::agent::context_window::prepare_messages_for_hierarchical_llm_sync(
            &mut messages,
            cfg,
        );
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
                let json_fragment =
                    super::super::manager_json_repair::extract_json_candidate_for_repair(content);
                let repair_user = Self::build_reflection_json_repair_user_prompt(
                    json_fragment.as_str(),
                    &parse_err.to_string(),
                );
                let fixed = super::super::manager_json_repair::one_shot_json_repair_llm_response(
                    &params,
                    cfg,
                    Some(Self::REFLECTION_JSON_REPAIR_TEMPERATURE),
                    LlmSeedOverride::FromConfig,
                    Self::force_manager_structured_json_mode,
                    json_fragment,
                    repair_user,
                )
                .await
                .map_err(ManagerError::LlmError)?;
                log::debug!(
                    target: "crabmate",
                    "[HIERARCHICAL] Manager: JSON repair response preview: {}",
                    truncate_for_log(fixed.as_str(), 500)
                );
                self.parse_reflection_output(failed_goal, fixed.as_str(), working_dir, true)
            }
            Err(e) => Err(e),
        }
    }

    /// 分层反思：为降低发散，使用略低于主配置的温度
    pub(super) const REFLECTION_PRIMARY_TEMPERATURE: f32 = 0.25;
    /// 仅修 JSON 的补调用：更低温以约束格式
    pub(super) const REFLECTION_JSON_REPAIR_TEMPERATURE: f32 = 0.0;
    /// 分解/重规划 JSON 修复补调用温度（仅修格式/枚举，不改计划语义）。
    pub(super) const MANAGER_JSON_REPAIR_TEMPERATURE: f32 = 0.0;

    /// 构建反思「结构化执行摘要」供模型与日志共用
    pub(super) fn build_reflection_structured_block(
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
    pub(super) fn build_reflection_prompt(
        &self,
        failed_goal: &SubGoal,
        verification_failure: &str,
        execution_result: &TaskResult,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        artifacts: &[Artifact],
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
    pub(super) fn build_reflection_json_repair_user_prompt(
        json_fragment: &str,
        parse_error: &str,
    ) -> String {
        let err_preview = truncate_for_log(parse_error, 500);
        let a = super::super::manager_json_repair::truncate_to_char_boundary(json_fragment, 12_000);
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
    pub(super) fn build_manager_json_repair_user_prompt(
        json_fragment: &str,
        parse_error: &str,
    ) -> String {
        let err_preview = truncate_for_log(parse_error, 500);
        let a = super::super::manager_json_repair::truncate_to_char_boundary(json_fragment, 12_000);
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
    pub(super) fn parse_reflection_output(
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
        let json_str =
            super::super::manager_json_repair::extract_json(content).ok_or_else(|| {
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
            consumes_from_dependencies: Option<Vec<super::super::task::DependencyContractEntry>>,
            required_tools: Option<Vec<String>>,
            #[serde(default)]
            goal_type: Option<super::super::task::GoalType>,
            #[serde(default)]
            build_requirements: Option<super::super::task::BuildRequirements>,
            #[serde(default)]
            acceptance: Option<super::super::task::GoalAcceptance>,
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
        super::super::subgoal_context::normalize_subgoal_io_contracts(&mut updated_goal);

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
