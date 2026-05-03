//! 同层子目标并行执行（`JoinSet` + 信号量），自 `execution_impl` 拆出以控制单文件行数。

use crate::agent::hierarchy::{
    HierarchicalExecutor,
    artifact_store::ArtifactStore,
    build_state::BuildState,
    execution_error::ExecutionError,
    task::{SubGoal, TaskResult, TaskStatus},
};
use log::{error, info, warn};
use std::collections::HashSet;

/// 单个并行子目标任务体（与 `execution_parallel.rs` 同级文件）。
#[path = "execution_parallel_spawn.rs"]
mod execution_parallel_spawn;

impl<'a> HierarchicalExecutor<'a> {
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
        let (Some(cfg), Some(client), Some(api_key)) = (
            self.cfg.as_ref(),
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

        let semaphore = Arc::new(Semaphore::new(self.max_parallel));
        let mut join_set = JoinSet::new();

        // 准备共享数据（使用 Arc 包装）
        let cfg = Arc::new(cfg.clone());
        let client = client.clone();
        let api_key = api_key.clone();
        let working_dir = self.working_dir.clone();
        let tools_defs = self.tools_defs.clone();
        let sse_out = self.sse_out.clone(); // 克隆 SSE 发送器以支持并行执行
        let tool_approval_out = self.tool_approval_out.clone(); // 克隆审批发送器
        let tool_approval_rx = self.tool_approval_rx.clone(); // 克隆审批接收器
        let hl = self
            .handler_lookup
            .clone()
            .expect("hierarchical executor missing handler_lookup (with_context not applied)");
        let sb = self.sync_default_sandbox_backend.clone().expect(
            "hierarchical executor missing sync_default_sandbox_backend (with_context not applied)",
        );
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

            let tools_defs_arc = Arc::new(tools_defs.clone());
            join_set.spawn(async move {
                let _permit = permit; // 持有 permit 直到任务完成
                execution_parallel_spawn::run_one_parallel_subgoal(
                    execution_parallel_spawn::ParallelSubgoalTask {
                        goal,
                        cfg,
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
        let mut results = Vec::new();
        let mut failed_count = 0;
        let mut panicked_count = 0;

        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((goal_id, Ok(result))) => {
                    // 将成功的结果产物合并到主 artifact_store
                    if matches!(result.status, TaskStatus::Completed) {
                        artifact_store.store_result(&result);
                        info!(
                            target: "crabmate",
                            "[HIERARCHICAL] Parallel: Goal {} completed successfully",
                            goal_id
                        );
                    } else if matches!(result.status, TaskStatus::NeedsDecomposition { .. }) {
                        // NeedsDecomposition 不是失败，而是需要重新规划
                        info!(
                            target: "crabmate",
                            "[HIERARCHICAL] Parallel: Goal {} needs decomposition",
                            goal_id
                        );
                    } else {
                        warn!(
                            target: "crabmate",
                            "[HIERARCHICAL] Parallel: Goal {} failed: {:?}",
                            goal_id, result.status
                        );
                    }
                    results.push(result);
                }
                Ok((goal_id, Err(e))) => {
                    // 任务执行出错，但继续收集其他结果
                    failed_count += 1;
                    error!(
                        target: "crabmate",
                        "[HIERARCHICAL] Parallel: Goal {} execution error: {}",
                        goal_id, e
                    );
                    // 创建一个失败的 TaskResult 而不是直接返回错误
                    results.push(TaskResult {
                        task_id: goal_id.clone(),
                        status: TaskStatus::Failed {
                            reason: format!("Execution error: {}", e),
                        },
                        output: None,
                        error: Some(format!("Execution error: {}", e)),
                        artifacts: Vec::new(),
                        duration_ms: 0,
                        tools_invoked: Vec::new(),
                    });
                }
                Err(e) => {
                    // 任务 panic，但继续收集其他结果
                    panicked_count += 1;
                    error!(
                        target: "crabmate",
                        "[HIERARCHICAL] Parallel: Task panicked: {}",
                        e
                    );
                    // 从 panic 信息中提取 goal_id（如果可能）
                    let goal_id = format!("unknown_panicked_{}", panicked_count);
                    results.push(TaskResult {
                        task_id: goal_id,
                        status: TaskStatus::Failed {
                            reason: format!("Task panicked: {}", e),
                        },
                        output: None,
                        error: Some(format!("Task panicked: {}", e)),
                        artifacts: Vec::new(),
                        duration_ms: 0,
                        tools_invoked: Vec::new(),
                    });
                }
            }
        }

        // 记录执行统计
        let completed_count = results
            .iter()
            .filter(|r| matches!(r.status, TaskStatus::Completed))
            .count();
        let needs_decomp_count = results
            .iter()
            .filter(|r| matches!(r.status, TaskStatus::NeedsDecomposition { .. }))
            .count();
        info!(
            target: "crabmate",
            "[HIERARCHICAL] Parallel execution summary: {} total, {} completed, {} needs_decomposition, {} failed, {} panicked",
            results.len(),
            completed_count,
            needs_decomp_count,
            failed_count,
            panicked_count
        );

        // 如果所有任务都失败了（排除 NeedsDecomposition），返回错误
        if completed_count == 0 && needs_decomp_count == 0 && !results.is_empty() {
            return Err(ExecutionError::MaxFailuresReached(format!(
                "All {} parallel tasks failed ({} execution errors, {} panics)",
                results.len(),
                failed_count,
                panicked_count
            )));
        }

        Ok(results)
    }
}
