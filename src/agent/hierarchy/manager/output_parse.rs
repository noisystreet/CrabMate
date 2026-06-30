//! 分解 JSON 解析与一次 JSON 修复补调用。

use crate::llm::{CompleteChatRetryingParams, LlmRetryingTransportOpts};
use crate::types::LlmSeedOverride;

use super::super::task::{ExecutionStrategy, SubGoal};
use super::manager_tail::{ManagerOutput, truncate_for_log, truncate_task};
use super::types::{ManagerAgent, ManagerError, ManagerLlmContext};

impl ManagerAgent {
    pub(super) fn parse_output(
        &self,
        content: &str,
        finish_reason: Option<&str>,
    ) -> Result<ManagerOutput, ManagerError> {
        let json_str = super::super::manager_json_repair::extract_json(content).ok_or_else(|| {
            let diag = super::super::manager_json_repair::extract_json_diagnostic(content);
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
            consumes_from_dependencies: Option<Vec<super::super::task::DependencyContractEntry>>,
            required_tools: Option<Vec<String>>,
            #[serde(default)]
            goal_type: Option<super::super::task::GoalType>,
            #[serde(default)]
            build_requirements: Option<super::super::task::BuildRequirements>,
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
            super::super::subgoal_context::normalize_subgoal_io_contracts(&mut g);
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
    pub(super) async fn parse_output_with_one_json_repair(
        &self,
        content: &str,
        finish_reason: Option<&str>,
        llm: ManagerLlmContext<'_>,
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
                let json_fragment =
                    super::super::manager_json_repair::extract_json_candidate_for_repair(content);
                let repair_user = Self::build_manager_json_repair_user_prompt(
                    json_fragment.as_str(),
                    &parse_err.to_string(),
                );
                let params = CompleteChatRetryingParams::new(
                    llm.llm_backend,
                    llm.client,
                    llm.api_key,
                    llm.cfg,
                    LlmRetryingTransportOpts::headless_no_stream(),
                    None,
                    None,
                )
                .with_turn_budget(llm.turn_budget);
                let fixed = super::super::manager_json_repair::one_shot_json_repair_llm_response(
                    &params,
                    llm.cfg,
                    Some(Self::MANAGER_JSON_REPAIR_TEMPERATURE),
                    LlmSeedOverride::FromConfig,
                    Self::force_manager_structured_json_mode,
                    json_fragment,
                    repair_user,
                )
                .await
                .map_err(ManagerError::LlmError)?;
                log::debug!(
                    target: "crabmate",
                    "[HIERARCHICAL] Manager: plan JSON repair response preview: {}",
                    truncate_for_log(fixed.as_str(), 500)
                );
                self.parse_output(fixed.as_str(), None)
            }
            Err(e) => Err(e),
        }
    }
}
