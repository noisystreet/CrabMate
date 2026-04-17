//! 分层 Agent 运行器
//!
//! 提供高层入口，封装 Router → Manager → Operator → Executor 流程

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;

use super::execution::{ExecutionError, HierarchicalExecutionResult};
use super::manager::ManagerAgent;
use super::router::Router;
use super::{AgentMode, ExecutionStrategy, HierarchicalExecutor, ManagerConfig};

/// 分层 Agent 运行参数
pub struct HierarchyRunnerParams<'a> {
    /// 任务描述
    pub task: &'a str,
    /// Agent 配置
    pub cfg: &'a AgentConfig,
    /// LLM 后端
    pub llm_backend: &'a dyn ChatCompletionsBackend,
    /// HTTP 客户端
    pub client: std::sync::Arc<reqwest::Client>,
    /// API 密钥
    pub api_key: String,
    /// 工作目录
    pub working_dir: std::path::PathBuf,
}

/// 分层 Agent 运行结果
#[derive(Debug)]
pub struct HierarchyRunnerResult {
    /// 执行结果
    pub execution_result: HierarchicalExecutionResult,
    /// 使用的 Agent 模式
    pub mode: AgentMode,
}

/// 运行分层 Agent（完整流程）
#[allow(dead_code)]
pub async fn run_hierarchical(
    params: HierarchyRunnerParams<'_>,
) -> Result<HierarchyRunnerResult, ExecutionError> {
    let HierarchyRunnerParams {
        task,
        cfg,
        llm_backend,
        client,
        api_key,
        working_dir,
    } = params;

    // 1. 路由决策
    let router_output = Router::route(task);
    log::info!(
        target: "crabmate",
        "Hierarchy runner: task={}, mode={:?}, max_sub_goals={}",
        truncate_string(task, 50),
        router_output.mode,
        router_output.max_sub_goals
    );

    // 如果不是 Hierarchical 或 MultiAgent 模式，降级到简单执行
    if !matches!(
        router_output.mode,
        AgentMode::Hierarchical | AgentMode::MultiAgent
    ) {
        log::info!(
            target: "crabmate",
            "Task complexity {} doesn't require hierarchical execution, falling back",
            router_output.mode.as_str()
        );
        return run_simple_fallback(task, cfg, llm_backend, client, api_key, working_dir).await;
    }

    // 2. Manager 分解任务
    let manager_config = ManagerConfig {
        max_sub_goals: router_output.max_sub_goals,
        execution_strategy: ExecutionStrategy::Hybrid,
        enabled: true,
    };
    let manager = ManagerAgent::new(manager_config);

    let manager_output = manager
        .decompose_with_llm(task, cfg, llm_backend, client.as_ref(), &api_key)
        .await
        .map_err(|e| ExecutionError::MaxFailuresReached(e.to_string()))?;

    log::info!(
        target: "crabmate",
        "Manager decomposed task into {} sub-goals, strategy={:?}",
        manager_output.sub_goals.len(),
        manager_output.execution_strategy
    );

    // 3. 执行子目标（传递完整上下文）
    let executor = HierarchicalExecutor::new(router_output.max_iterations, 3).with_context(
        llm_backend,
        cfg.clone(),
        client.clone(),
        api_key.clone(),
        working_dir.clone(),
    );
    let execution_result = executor.execute_with_result(manager_output.clone()).await?;

    Ok(HierarchyRunnerResult {
        execution_result,
        mode: router_output.mode,
    })
}

/// 简单降级执行（不进行任务分解）
async fn run_simple_fallback(
    task: &str,
    cfg: &AgentConfig,
    llm_backend: &dyn ChatCompletionsBackend,
    client: std::sync::Arc<reqwest::Client>,
    api_key: String,
    working_dir: std::path::PathBuf,
) -> Result<HierarchyRunnerResult, ExecutionError> {
    // 直接使用 Manager 的降级分解
    let manager_config = ManagerConfig::default();
    let manager = ManagerAgent::new(manager_config);

    let manager_output = manager
        .decompose_with_llm(task, cfg, llm_backend, client.as_ref(), &api_key)
        .await
        .map_err(|e| ExecutionError::MaxFailuresReached(e.to_string()))?;

    let executor = HierarchicalExecutor::new(10, 3).with_context(
        llm_backend,
        cfg.clone(),
        client,
        api_key,
        working_dir,
    );
    let execution_result = executor.execute_with_result(manager_output).await?;

    Ok(HierarchyRunnerResult {
        execution_result,
        mode: AgentMode::Single,
    })
}

/// 截断字符串
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

impl AgentMode {
    /// 获取模式名称
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentMode::Single => "single",
            AgentMode::ReAct => "react",
            AgentMode::Hierarchical => "hierarchical",
            AgentMode::MultiAgent => "multi_agent",
        }
    }
}
