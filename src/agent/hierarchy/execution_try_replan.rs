//! [`super::HierarchicalExecutor::try_replan`]（预留接口）拆至此文件以降低 `execution_impl` 圈复杂度。

use super::super::artifact_store::ArtifactStore;
use super::super::execution_error::ExecutionError;
use super::super::manager::{ManagerAgent, ManagerLlmContext};
use super::super::task::{SubGoal, TaskResult};
use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;

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
        let err = |msg: &'static str| ExecutionError::MaxFailuresReached(msg.to_string());
        Ok(ReplanInputs {
            manager: self
                .manager
                .as_ref()
                .ok_or_else(|| err("No manager available for replanning"))?,
            original_task: self
                .original_task
                .as_ref()
                .ok_or_else(|| err("No original task available for replanning"))?,
            working_dir: self
                .working_dir
                .as_ref()
                .ok_or_else(|| err("No working_dir available for replanning"))?,
            cfg: self
                .cfg
                .as_ref()
                .ok_or_else(|| err("No cfg available for replanning"))?,
            llm_backend: self
                .llm_backend
                .ok_or_else(|| err("No llm_backend available for replanning"))?,
            client: self
                .client
                .as_ref()
                .ok_or_else(|| err("No client available for replanning"))?,
            api_key: self
                .api_key
                .as_ref()
                .ok_or_else(|| err("No api_key available for replanning"))?,
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
