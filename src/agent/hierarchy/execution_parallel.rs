//! 同层子目标并行执行（`JoinSet` + 信号量），自 `execution_impl` 拆出以控制单文件行数。

use crate::agent::hierarchy::{
    HierarchicalExecutor,
    artifact_store::ArtifactStore,
    build_state::BuildState,
    execution_error::ExecutionError,
    task::{SubGoal, TaskResult},
};
use log::warn;
use std::collections::HashSet;

/// 单个并行子目标任务体（与 `execution_parallel.rs` 同级文件）。
#[path = "execution_parallel_spawn.rs"]
mod execution_parallel_spawn;

#[path = "execution_parallel_collect.rs"]
mod execution_parallel_collect;

impl HierarchicalExecutor {
    /// 并行执行
    ///
    /// 使用 tokio::spawn 实现真正的并发执行，通过信号量控制并发度
    ///
    /// 特性：
    /// - 支持部分失败继续执行（收集所有结果后返回）
    /// - 使用 JoinSet 实现进度追踪和实时结果收集
    /// - 每个子目标使用独立的 ArtifactStore，执行完成后合并结果
    pub(in crate::agent::hierarchy::execution) async fn execute_parallel(
        &self,
        goals: &[&SubGoal],
        prior_subgoal_results: &[TaskResult],
        artifact_store: &mut ArtifactStore,
        build_state: &mut BuildState,
    ) -> Result<Vec<TaskResult>, ExecutionError> {
        use std::sync::Arc;
        use tokio::sync::Semaphore;
        use tokio::task::JoinSet;

        // 如果没有配置，回退到顺序执行
        let (Some(cfg), Some(llm_backend), Some(client), Some(api_key)) = (
            self.cfg.as_ref(),
            self.llm_backend.as_ref(),
            self.client.as_ref(),
            self.api_key.as_ref(),
        ) else {
            warn!(
                target: "crabmate",
                "[HIERARCHICAL] Parallel execution requires full context, falling back to sequential"
            );
            return self
                .execute_sequential(goals, prior_subgoal_results, artifact_store, build_state)
                .await;
        };

        if let Some(reason) = crate::agent::hierarchy::turn_abort::hierarchical_abort_reason(
            self.sse_out.as_ref(),
            self.cancel.as_deref(),
        ) {
            return Err(ExecutionError::TurnAborted(reason));
        }

        let semaphore = Arc::new(Semaphore::new(self.max_parallel));
        let mut join_set = JoinSet::new();

        // 准备共享数据（使用 Arc 包装）
        let cfg = Arc::new(cfg.clone());
        let llm_backend = Arc::clone(llm_backend);
        let client = client.clone();
        let api_key = api_key.clone();
        let working_dir = self.working_dir.clone();
        let tools_defs = self.tools_defs.clone();
        let sse_out = self.sse_out.clone(); // 克隆 SSE 发送器以支持并行执行
        let tool_approval_out = self.tool_approval_out.clone(); // 克隆审批发送器
        let tool_approval_rx = self.tool_approval_rx.clone(); // 克隆审批接收器
        let cancel = self.cancel.clone();
        let (hl, sb) = self.require_tool_dispatch_handles()?;
        let probe_cache = self.probe_cache.clone();
        let prior = Arc::new(prior_subgoal_results.to_vec());
        let pre_snapshot: Arc<ArtifactStore> = Arc::new(artifact_store.clone());
        let current_ids: std::sync::Arc<HashSet<String>> =
            Arc::new(goals.iter().map(|g| g.goal_id.to_string()).collect());

        // 为每个子目标创建并发任务
        for goal in goals {
            let permit = semaphore.clone().acquire_owned().await.map_err(|e| {
                ExecutionError::DagError(format!("Failed to acquire semaphore: {}", e))
            })?;

            let goal = (*goal).clone();
            let cfg = cfg.clone();
            let client = client.clone();
            let api_key = api_key.clone();
            let working_dir = working_dir.clone();
            let tools_defs = tools_defs.clone();
            let build_state = build_state.clone();
            let sse_out = sse_out.clone(); // 每个任务克隆 SSE 发送器
            let sse_out_for_start = sse_out.clone();
            let tool_approval_out = tool_approval_out.clone(); // 每个任务克隆审批发送器
            let tool_approval_rx = tool_approval_rx.clone(); // 每个任务克隆审批接收器
            let probe_cache = probe_cache.clone();
            let prior = prior.clone();
            let pre_snapshot = pre_snapshot.clone();
            let current_ids = current_ids.clone();
            let hl_c = hl.clone();
            let sb_c = sb.clone();
            let llm_backend = Arc::clone(&llm_backend);
            let cancel = cancel.clone();
            let turn_budget = self.turn_budget.clone();

            let tools_defs_arc = Arc::new(tools_defs.clone());
            join_set.spawn(async move {
                let _permit = permit; // 持有 permit 直到任务完成
                execution_parallel_spawn::run_one_parallel_subgoal(
                    execution_parallel_spawn::ParallelSubgoalTask {
                        goal,
                        cfg,
                        llm_backend,
                        client,
                        api_key,
                        working_dir,
                        tools_defs: tools_defs_arc,
                        build_state,
                        sse_out_operator: sse_out,
                        sse_out_timeline: sse_out_for_start,
                        tool_approval_out,
                        tool_approval_rx,
                        probe_cache,
                        cancel,
                        turn_budget,
                        prior,
                        pre_snapshot,
                        current_ids,
                        handler_lookup: hl_c,
                        sync_default_sandbox_backend: sb_c,
                    },
                )
                .await
            });
        }

        // 使用 JoinSet 收集所有结果（支持部分失败继续）
        let (results, failed_count, panicked_count) =
            execution_parallel_collect::drain_parallel_join_set(
                &mut join_set,
                artifact_store,
                self.sse_out.as_ref(),
                self.cancel.as_ref(),
            )
            .await?;

        execution_parallel_collect::finalize_parallel_results(results, failed_count, panicked_count)
    }
}
