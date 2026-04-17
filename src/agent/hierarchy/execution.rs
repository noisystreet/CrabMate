//! 分层执行器：按依赖层级执行子目标

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;

use super::artifact_store::ArtifactStore;
use super::manager::{ManagerOutput, handle_failure};
use super::operator::{OperatorAgent, OperatorConfig};
use super::task::{ExecutionStrategy, SubGoal, TaskResult, TaskStatus};
use super::tool_executor::ToolExecutor;
use log::{error, info};

/// 分层执行结果
#[derive(Debug, Clone)]
pub struct HierarchicalExecutionResult {
    pub results: Vec<TaskResult>,
    pub total_duration_ms: u64,
    pub total_completed: usize,
    pub total_failed: usize,
}

/// 分层执行器错误
#[derive(Debug)]
pub enum ExecutionError {
    DagError(String),
    MaxFailuresReached(String),
    OperatorError(super::operator::OperatorError),
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionError::DagError(s) => write!(f, "DAG error: {}", s),
            ExecutionError::MaxFailuresReached(s) => write!(f, "Max failures: {}", s),
            ExecutionError::OperatorError(e) => write!(f, "Operator error: {}", e),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<super::operator::OperatorError> for ExecutionError {
    fn from(e: super::operator::OperatorError) -> Self {
        ExecutionError::OperatorError(e)
    }
}

/// 分层执行器
pub struct HierarchicalExecutor<'a> {
    max_parallel: usize,
    max_failures: usize,
    /// LLM 后端（用于 Operator 的 ReAct 循环）
    llm_backend: Option<&'a dyn ChatCompletionsBackend>,
    /// Agent 配置
    cfg: Option<AgentConfig>,
    /// HTTP 客户端
    client: Option<std::sync::Arc<reqwest::Client>>,
    /// API 密钥
    api_key: Option<String>,
    /// 工作目录
    working_dir: Option<std::path::PathBuf>,
}

impl HierarchicalExecutor<'_> {
    pub fn new(max_parallel: usize, max_failures: usize) -> Self {
        Self {
            max_parallel,
            max_failures,
            llm_backend: None,
            cfg: None,
            client: None,
            api_key: None,
            working_dir: None,
        }
    }
}

impl<'a> HierarchicalExecutor<'a> {
    /// 设置执行上下文
    pub fn with_context(
        mut self,
        llm_backend: &'a dyn ChatCompletionsBackend,
        cfg: AgentConfig,
        client: std::sync::Arc<reqwest::Client>,
        api_key: String,
        working_dir: std::path::PathBuf,
    ) -> Self {
        self.llm_backend = Some(llm_backend);
        self.cfg = Some(cfg);
        self.client = Some(client);
        self.api_key = Some(api_key);
        self.working_dir = Some(working_dir);
        self
    }

    /// 执行并返回详细结果
    pub async fn execute_with_result(
        &self,
        manager_output: ManagerOutput,
    ) -> Result<HierarchicalExecutionResult, ExecutionError> {
        let start_time = Instant::now();
        let sub_goals = manager_output.sub_goals;
        let strategy = manager_output.execution_strategy;

        info!(
            target: "crabmate",
            "Hierarchical execution started: {} goals, strategy={:?}",
            sub_goals.len(),
            strategy
        );

        if sub_goals.is_empty() {
            return Ok(HierarchicalExecutionResult {
                results: Vec::new(),
                total_duration_ms: start_time.elapsed().as_millis() as u64,
                total_completed: 0,
                total_failed: 0,
            });
        }

        // 构建 DAG
        let dag = Dag::build(&sub_goals)?;

        // 计算拓扑层级
        let levels = dag.topological_levels()?;

        info!(
            target: "crabmate",
            "Hierarchical execution: {} goals in {} levels",
            sub_goals.len(),
            levels.len()
        );

        let mut artifact_store = ArtifactStore::new();
        let mut all_results = Vec::new();

        // 按层级执行
        for (level_idx, level) in levels.iter().enumerate() {
            info!(target: "crabmate", "Executing level {} with {} goals", level_idx, level.len());

            // 获取该层级的子目标
            let level_goals: Vec<_> = level
                .iter()
                .filter_map(|id| sub_goals.iter().find(|g| &g.goal_id == id))
                .collect();

            // 按策略执行
            let level_results = match strategy {
                ExecutionStrategy::Sequential => {
                    self.execute_sequential(&level_goals, &mut artifact_store)
                        .await
                }
                ExecutionStrategy::Parallel | ExecutionStrategy::Hybrid => {
                    self.execute_parallel(&level_goals, &mut artifact_store)
                        .await
                }
            }?;

            // 更新 artifact store
            for result in &level_results {
                if matches!(result.status, TaskStatus::Completed) {
                    artifact_store.store_result(result);
                }
                all_results.push(result.clone());
            }

            // 检查失败
            let (_, failed, decision) = handle_failure(&all_results, self.max_failures);
            if !failed.is_empty()
                && let super::manager::FailureDecision::Abort { .. } = decision
            {
                error!(
                    target: "crabmate",
                    "Max failures reached at level {}, aborting",
                    level_idx
                );
                return Err(ExecutionError::MaxFailuresReached(format!(
                    "Failed {} goals, exceeding threshold",
                    failed.len()
                )));
            }
        }

        let total_duration_ms = start_time.elapsed().as_millis() as u64;
        let total_completed = all_results
            .iter()
            .filter(|r| matches!(r.status, TaskStatus::Completed))
            .count();
        let total_failed = all_results
            .iter()
            .filter(|r| matches!(r.status, TaskStatus::Failed { .. }))
            .count();

        info!(
            target: "crabmate",
            "Hierarchical execution finished: {} completed, {} failed, {}ms",
            total_completed,
            total_failed,
            total_duration_ms
        );

        Ok(HierarchicalExecutionResult {
            results: all_results,
            total_duration_ms,
            total_completed,
            total_failed,
        })
    }

    /// 执行子目标列表（保持原有接口兼容）
    pub async fn execute(
        &self,
        manager_output: ManagerOutput,
    ) -> Result<Vec<TaskResult>, ExecutionError> {
        let result = self.execute_with_result(manager_output).await?;
        Ok(result.results)
    }

    /// 顺序执行
    async fn execute_sequential(
        &self,
        goals: &[&SubGoal],
        artifact_store: &mut ArtifactStore,
    ) -> Result<Vec<TaskResult>, ExecutionError> {
        let mut results = Vec::new();

        for goal in goals {
            let result = self.execute_single(goal, artifact_store).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// 并行执行
    async fn execute_parallel(
        &self,
        goals: &[&SubGoal],
        _artifact_store: &mut ArtifactStore,
    ) -> Result<Vec<TaskResult>, ExecutionError> {
        let mut results = Vec::new();

        // 分块执行（简化版：顺序执行）
        for chunk in goals.chunks(self.max_parallel) {
            for goal in chunk {
                let mut store = ArtifactStore::new();
                let result = self.execute_single(goal, &mut store).await?;
                results.push(result);
            }
        }

        Ok(results)
    }

    /// 执行单个子目标
    async fn execute_single(
        &self,
        goal: &SubGoal,
        artifact_store: &mut ArtifactStore,
    ) -> Result<TaskResult, ExecutionError> {
        // 获取依赖的 artifacts
        let deps = artifact_store.get_dependencies(&goal.depends_on);

        info!(
            target: "crabmate",
            "Executing goal {} with {} dependencies",
            goal.goal_id,
            deps.len()
        );

        // 构建 Operator 配置
        let allowed_tools =
            super::operator::get_tools_for_capabilities(&goal.required_capabilities);
        let op_config = OperatorConfig {
            max_iterations: 10,
            allowed_tools,
        };

        let operator = OperatorAgent::new(op_config);

        // 根据是否有完整上下文选择执行方法
        let result =
            if let (Some(llm_backend), Some(cfg), Some(client), Some(api_key), Some(work_dir)) = (
                self.llm_backend,
                self.cfg.as_ref(),
                self.client.as_ref(),
                self.api_key.as_ref(),
                self.working_dir.as_ref(),
            ) {
                // 有完整上下文，使用带工具的执行
                let tool_executor = ToolExecutor::new(cfg, work_dir.clone());
                operator
                    .execute_with_tools(goal, cfg, llm_backend, client, api_key, &tool_executor)
                    .await
            } else {
                // 降级使用简化版本
                operator.execute(goal).await
            };

        let result = result.map_err(ExecutionError::OperatorError)?;

        // 如果完成，存储 artifacts
        if matches!(result.status, super::task::TaskStatus::Completed) {
            artifact_store.store_result(&result);
        }

        Ok(result)
    }
}

/// DAG 构建器
#[derive(Debug)]
struct Dag {
    nodes: HashSet<String>,
    edges: HashMap<String, HashSet<String>>,
}

impl Dag {
    fn build(goals: &[SubGoal]) -> Result<Self, ExecutionError> {
        let mut dag = Dag {
            nodes: HashSet::new(),
            edges: HashMap::new(),
        };

        for goal in goals {
            dag.nodes.insert(goal.goal_id.clone());
            dag.edges.entry(goal.goal_id.clone()).or_default();

            for dep in &goal.depends_on {
                if !dag.nodes.contains(dep) {
                    dag.nodes.insert(dep.clone());
                    dag.edges.entry(dep.clone()).or_default();
                }
                dag.edges.get_mut(dep).unwrap().insert(goal.goal_id.clone());
            }
        }

        Ok(dag)
    }

    /// 计算拓扑层级
    fn topological_levels(&self) -> Result<Vec<Vec<String>>, ExecutionError> {
        let mut levels = Vec::new();
        let mut remaining = self.nodes.clone();
        let mut in_degree: HashMap<String, usize> =
            self.nodes.iter().map(|id| (id.clone(), 0)).collect();

        // 计算入度
        for targets in self.edges.values() {
            for target in targets {
                if let Some(d) = in_degree.get_mut(target) {
                    *d += 1;
                }
            }
        }

        while !remaining.is_empty() {
            // 找到入度为 0 的节点
            let level: Vec<String> = remaining
                .iter()
                .filter(|id| in_degree.get(*id) == Some(&0))
                .cloned()
                .collect();

            if level.is_empty() {
                return Err(ExecutionError::DagError(
                    "Cycle detected in dependencies".to_string(),
                ));
            }

            for id in &level {
                remaining.remove(id);
                if let Some(targets) = self.edges.get(id) {
                    for target in targets {
                        if let Some(d) = in_degree.get_mut(target) {
                            *d -= 1;
                        }
                    }
                }
            }

            levels.push(level);
        }

        Ok(levels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_levels() {
        let goals = vec![
            SubGoal::new("a", "task a"),
            SubGoal::new("b", "task b").with_depends_on(vec!["a".to_string()]),
            SubGoal::new("c", "task c").with_depends_on(vec!["a".to_string()]),
            SubGoal::new("d", "task d").with_depends_on(vec!["b".to_string(), "c".to_string()]),
        ];

        let dag = Dag::build(&goals).unwrap();
        let levels = dag.topological_levels().unwrap();

        assert_eq!(levels.len(), 3);
        assert!(levels[0].contains(&"a".to_string()));
        assert!(levels[1].contains(&"b".to_string()) || levels[1].contains(&"c".to_string()));
    }

    #[test]
    fn test_dag_cycle_detection() {
        let goals = vec![
            SubGoal::new("a", "task a").with_depends_on(vec!["b".to_string()]),
            SubGoal::new("b", "task b").with_depends_on(vec!["a".to_string()]),
        ];

        let dag = Dag::build(&goals).unwrap();
        let result = dag.topological_levels();

        assert!(result.is_err());
    }
}
