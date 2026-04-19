//! 分层 Agent 运行器
//!
//! 提供高层入口，封装 Router → Manager → Operator → Executor 流程

use tokio::sync::mpsc::Sender;

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::sse;

use super::events;
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
    /// SSE 发送器（用于发射分层执行进度事件）
    pub sse_out: Option<Sender<String>>,
    /// 工具定义列表（用于 Manager 分解）
    pub tools_defs: &'a [crate::types::Tool],
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
        sse_out,
        tools_defs,
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
        return run_simple_fallback(
            task,
            cfg,
            llm_backend,
            client,
            api_key,
            working_dir,
            sse_out,
            tools_defs,
        )
        .await;
    }

    // 发射 SSE 事件：Manager 开始
    log::info!(target: "crabmate", "[HIERARCHICAL] run_hierarchical: sse_out is {:?}", sse_out.is_some());
    if let Some(ref sse_out) = sse_out {
        let trace = events::build_manager_started_trace(task);
        let encoded = sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace });
        log::info!(target: "crabmate", "[HIERARCHICAL] manager_started encoded length={}", encoded.len());
        let _ = sse::send_string_logged(sse_out, encoded, "hierarchical::manager_started").await;
        log::info!(target: "crabmate", "[HIERARCHICAL] manager_started send completed");
    }

    // 2. Manager 分解任务
    let manager_config = ManagerConfig {
        max_sub_goals: router_output.max_sub_goals,
        execution_strategy: ExecutionStrategy::Hybrid,
        enabled: true,
    };
    let manager = ManagerAgent::new(manager_config);

    let manager_output = manager
        .decompose_with_llm(
            task,
            cfg,
            llm_backend,
            client.as_ref(),
            &api_key,
            tools_defs,
        )
        .await
        .map_err(|e| ExecutionError::MaxFailuresReached(e.to_string()))?;

    log::info!(
        target: "crabmate",
        "Manager decomposed task into {} sub-goals, strategy={:?}",
        manager_output.sub_goals.len(),
        manager_output.execution_strategy
    );

    // 发射 SSE 事件顺序：TimelineLog(Manager规划) → ThinkingTrace(manager_finished)
    if let Some(ref sse_out) = sse_out {
        // 生成子目标列表详情用于聊天气泡显示
        let sub_goals_detail = manager_output
            .sub_goals
            .iter()
            .map(|sg| format!("- [ ] {}: {}", sg.goal_id, sg.description))
            .collect::<Vec<_>>()
            .join("\n");
        let plan_summary = format!(
            "**Manager 规划** ({} 个子目标, 策略={})\n\n{}\n\n执行中...",
            manager_output.sub_goals.len(),
            manager_output.execution_strategy.as_str(),
            sub_goals_detail
        );
        let timeline_payload = crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "hierarchical_plan".to_string(),
                title: plan_summary.clone(),
                detail: None,
            },
        };
        let encoded = sse::encode_message(timeline_payload);
        let _ =
            sse::send_string_logged(sse_out, encoded, "hierarchical::manager_plan_timeline").await;

        // ThinkingTrace(manager_finished)
        let trace = events::build_manager_finished_trace(
            manager_output.sub_goals.len(),
            manager_output.execution_strategy.as_str(),
        );
        let _ = sse::send_string_logged(
            sse_out,
            sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
            "hierarchical::manager_finished",
        )
        .await;
    }

    // 3. 执行子目标（传递完整上下文）
    let mut executor = HierarchicalExecutor::new(router_output.max_iterations, 3)
        .with_context(
            llm_backend,
            cfg.clone(),
            client.clone(),
            api_key.clone(),
            working_dir.clone(),
        )
        .with_tools_defs(tools_defs.to_vec());
    if let Some(sse_tx) = sse_out {
        executor = executor.with_sse(sse_tx);
    }
    let execution_result = executor.execute_with_result(manager_output.clone()).await?;

    Ok(HierarchyRunnerResult {
        execution_result,
        mode: router_output.mode,
    })
}

/// 简单降级执行（不进行任务分解）
#[allow(clippy::too_many_arguments)]
async fn run_simple_fallback(
    task: &str,
    cfg: &AgentConfig,
    llm_backend: &dyn ChatCompletionsBackend,
    client: std::sync::Arc<reqwest::Client>,
    api_key: String,
    working_dir: std::path::PathBuf,
    sse_out: Option<Sender<String>>,
    tools_defs: &[crate::types::Tool],
) -> Result<HierarchyRunnerResult, ExecutionError> {
    // 直接使用 Manager 的降级分解
    let manager_config = ManagerConfig::default();
    let manager = ManagerAgent::new(manager_config);

    let manager_output = manager
        .decompose_with_llm(
            task,
            cfg,
            llm_backend,
            client.as_ref(),
            &api_key,
            tools_defs,
        )
        .await
        .map_err(|e| ExecutionError::MaxFailuresReached(e.to_string()))?;

    // 发送 SSE 事件顺序：1) TimelineLog(Manager规划) → 2) ThinkingTrace(manager_started) → 3) ThinkingTrace(manager_finished)
    // 这样前端 pending 队列按到达顺序就是正确的逻辑顺序
    if let Some(ref sse_out) = sse_out {
        // 1) Manager 规划（TimelineLog）
        let sub_goals_detail = manager_output
            .sub_goals
            .iter()
            .map(|sg| format!("- [ ] {}: {}", sg.goal_id, sg.description))
            .collect::<Vec<_>>()
            .join("\n");
        let plan_summary = format!(
            "**Manager 规划** ({} 个子目标, 策略={})\n\n{}\n\n执行中...",
            manager_output.sub_goals.len(),
            manager_output.execution_strategy.as_str(),
            sub_goals_detail
        );
        let timeline_payload = crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "hierarchical_plan".to_string(),
                title: plan_summary.clone(),
                detail: None,
            },
        };
        let encoded = sse::encode_message(timeline_payload);
        let _ =
            sse::send_string_logged(sse_out, encoded, "hierarchical::manager_plan_timeline").await;

        // 2) Manager 开始（ThinkingTrace）
        let trace = events::build_manager_started_trace(task);
        let encoded = sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace });
        let _ = sse::send_string_logged(sse_out, encoded, "hierarchical::manager_started").await;

        // 3) Manager 完成（ThinkingTrace）
        let trace = events::build_manager_finished_trace(
            manager_output.sub_goals.len(),
            manager_output.execution_strategy.as_str(),
        );
        let encoded_trace = sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace });
        let _ =
            sse::send_string_logged(sse_out, encoded_trace, "hierarchical::manager_finished").await;

        // 通知前端进入终答阶段
        let encoded_phase = sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
            assistant_answer_phase: true,
        });
        let _ = sse::send_string_logged(
            sse_out,
            encoded_phase,
            "hierarchical::assistant_answer_phase",
        )
        .await;
    }

    let mut executor = HierarchicalExecutor::new(10, 3)
        .with_context(llm_backend, cfg.clone(), client, api_key, working_dir)
        .with_tools_defs(tools_defs.to_vec());
    if let Some(sse_tx) = sse_out {
        executor = executor.with_sse(sse_tx);
    }
    let execution_result = executor.execute_with_result(manager_output).await?;

    Ok(HierarchyRunnerResult {
        execution_result,
        mode: AgentMode::Single,
    })
}

/// 截断字符串（按字符边界截断，支持中文）
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated = s
            .char_indices()
            .take(max_len.saturating_sub(3))
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &s[..truncated])
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
