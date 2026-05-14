//! [`super::HierarchicalExecutor::try_replan`]（预留接口）拆至此文件以降低 `execution_impl` 圈复杂度。

use super::super::artifact_store::ArtifactStore;
use super::super::execution_error::ExecutionError;
use super::super::manager::{ManagerAgent, ManagerLlmContext};
use super::super::task::{SubGoal, TaskResult};
use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;

fn require_replan_field<T>(value: Option<T>, missing: &'static str) -> Result<T, ExecutionError> {
    value.ok_or_else(|| ExecutionError::MaxFailuresReached(missing.to_string()))
}

struct ReplanInputs<'a> {
    manager: &'a ManagerAgent,
    original_task: &'a str,
    working_dir: &'a std::path::PathBuf,
    cfg: &'a AgentConfig,
    llm_backend: &'a dyn ChatCompletionsBackend,
    client: &'a reqwest::Client,
    api_key: &'a str,
}

impl<'a> super::HierarchicalExecutor<'a> {
    fn gather_replan_inputs(&'a self) -> Result<ReplanInputs<'a>, ExecutionError> {
        Ok(ReplanInputs {
            manager: require_replan_field(
                self.manager.as_ref(),
                "No manager available for replanning",
            )?,
            original_task: require_replan_field(
                self.original_task.as_ref(),
                "No original task available for replanning",
            )?,
            working_dir: require_replan_field(
                self.working_dir.as_ref(),
                "No working_dir available for replanning",
            )?,
            cfg: require_replan_field(self.cfg.as_ref(), "No cfg available for replanning")?,
            llm_backend: require_replan_field(
                self.llm_backend,
                "No llm_backend available for replanning",
            )?,
            client: require_replan_field(
                self.client.as_ref(),
                "No client available for replanning",
            )?,
            api_key: require_replan_field(
                self.api_key.as_ref(),
                "No api_key available for replanning",
            )?,
        })
    }

    /// 尝试基于已完成的结果和产物重新规划（预留接口）
    #[allow(dead_code)]
    pub(super) async fn try_replan(
        &self,
        previous_results: &[TaskResult],
        artifact_store: &ArtifactStore,
    ) -> Result<Vec<SubGoal>, ExecutionError> {
        let g = self.gather_replan_inputs()?;
        let artifacts: Vec<_> = artifact_store.all().into_iter().cloned().collect();

        let manager_output = g
            .manager
            .replan_with_artifacts(
                g.original_task,
                ManagerLlmContext {
                    cfg: g.cfg,
                    llm_backend: g.llm_backend,
                    client: g.client,
                    api_key: g.api_key,
                },
                g.working_dir,
                &self.tools_defs,
                previous_results,
                &artifacts,
            )
            .await
            .map_err(|e| ExecutionError::MaxFailuresReached(e.to_string()))?;

        Ok(manager_output.sub_goals)
    }
}
