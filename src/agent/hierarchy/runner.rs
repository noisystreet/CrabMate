//! 分层 Agent 运行器
//!
//! 提供高层入口，封装 Router → Manager → Operator → Executor 流程

use std::sync::Arc;
use tokio::sync::{
    Mutex,
    mpsc::{Receiver, Sender},
};

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::sse;
use crate::types::CommandApprovalDecision;

use super::events;
use super::execution::{ExecutionError, HierarchicalExecutionResult};
use super::manager::ManagerAgent;
use super::router::SmartRouter;
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
    /// 工具审批发送器（用于触发审批对话框）
    pub tool_approval_out: Option<Sender<String>>,
    /// 工具审批接收器（用于接收用户审批决定）
    pub tool_approval_rx: Option<Arc<Mutex<Receiver<CommandApprovalDecision>>>>,
    /// 一期意图识别：主意图标签（如 execute.run_test_build）
    pub primary_intent: Option<String>,
    /// 一期意图识别：次意图标签列表
    pub secondary_intents: Vec<String>,
    /// 是否启用基于意图标签的执行模式偏置。
    pub intent_mode_bias_enabled: bool,
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
        tool_approval_out,
        tool_approval_rx,
        primary_intent,
        secondary_intents,
        intent_mode_bias_enabled,
    } = params;

    // 1. 智能路由决策
    // 默认使用规则路由，可以通过配置启用 LLM 智能路由
    let use_llm_routing = cfg.enable_llm_routing.unwrap_or(false);
    let router = SmartRouter::new();
    let mut router_output = router
        .route_smart(
            task,
            cfg,
            llm_backend,
            client.as_ref(),
            &api_key,
            use_llm_routing,
        )
        .await;
    if intent_mode_bias_enabled {
        apply_intent_mode_bias(
            &mut router_output,
            primary_intent.as_deref(),
            &secondary_intents,
        );
    }

    log::info!(
        target: "crabmate",
        "Hierarchy runner: task={}, mode={:?}, strategy={:?}, max_sub_goals={}",
        truncate_string(task, 50),
        router_output.mode,
        router_output.routing_strategy,
        router_output.max_sub_goals
    );

    // 记录路由决策理由到日志
    if let Some(ref reasoning) = router_output.reasoning {
        log::info!(
            target: "crabmate",
            "[ROUTER] Decision reasoning: {}",
            reasoning
        );
    }

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
            tool_approval_out,
            tool_approval_rx,
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
            &working_dir,
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
            "**Manager 规划** ({} 个子目标, 策略={})\n\n{}",
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
        .with_tools_defs(tools_defs.to_vec())
        .with_manager(manager.clone())
        .with_original_task(task.to_string());
    if let Some(sse_tx) = sse_out {
        executor = executor.with_sse(sse_tx);
    }
    // 如果有审批上下文，传递给 executor
    if let (Some(out_tx), Some(approval_rx)) = (tool_approval_out, tool_approval_rx) {
        executor = executor.with_tool_approval(out_tx, approval_rx);
    }
    let execution_result = executor.execute_with_result(manager_output.clone()).await?;

    Ok(HierarchyRunnerResult {
        execution_result,
        mode: router_output.mode,
    })
}

fn apply_intent_mode_bias(
    router_output: &mut super::router::RouterOutput,
    primary_intent: Option<&str>,
    secondary_intents: &[String],
) {
    let mut intents = secondary_intents
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if let Some(primary) = primary_intent {
        intents.push(primary);
    }

    let has = |needle: &str| intents.contains(&needle);
    let prefer_hierarchical = has("execute.run_test_build")
        || has("execute.debug_diagnose")
        || has("execute.code_change");
    let prefer_multi_agent = has("execute.run_test_build") && has("execute.git_ops");

    if prefer_multi_agent && router_output.mode != AgentMode::MultiAgent {
        router_output.mode = AgentMode::MultiAgent;
        router_output.max_iterations = 50;
        router_output.max_sub_goals = 50;
        router_output.execution_strategy = ExecutionStrategy::Parallel;
        let reason =
            "意图偏置：检测到构建/测试与 Git 流程复合请求，提升为多 Agent 并行模式".to_string();
        router_output.reasoning = Some(match &router_output.reasoning {
            Some(old) => format!("{old}；{reason}"),
            None => reason,
        });
        return;
    }

    if prefer_hierarchical && matches!(router_output.mode, AgentMode::Single | AgentMode::ReAct) {
        router_output.mode = AgentMode::Hierarchical;
        router_output.max_iterations = 30;
        router_output.max_sub_goals = 20;
        router_output.execution_strategy = ExecutionStrategy::Hybrid;
        let reason = "意图偏置：检测到调试/构建/代码修改请求，提升为分层执行模式".to_string();
        router_output.reasoning = Some(match &router_output.reasoning {
            Some(old) => format!("{old}；{reason}"),
            None => reason,
        });
    }
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
    tool_approval_out: Option<Sender<String>>,
    tool_approval_rx: Option<Arc<Mutex<Receiver<CommandApprovalDecision>>>>,
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
            &working_dir,
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
            "**Manager 规划** ({} 个子目标, 策略={})\n\n{}",
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
        // 不在此下发 `assistant_answer_phase`：须等子目标进度或最终汇总，由 execution / handle_execution_result 统一发送，避免「进入终答相却无正文」的错位。
    }

    let mut executor = HierarchicalExecutor::new(10, 3)
        .with_context(llm_backend, cfg.clone(), client, api_key, working_dir)
        .with_tools_defs(tools_defs.to_vec());
    if let Some(sse_tx) = sse_out {
        executor = executor.with_sse(sse_tx);
    }
    // 如果有审批上下文，传递给 executor
    if let (Some(out_tx), Some(approval_rx)) = (tool_approval_out, tool_approval_rx) {
        executor = executor.with_tool_approval(out_tx, approval_rx);
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

#[cfg(test)]
mod tests {
    use super::apply_intent_mode_bias;
    use crate::agent::hierarchy::router::{AgentMode, RouterOutput, RoutingStrategy};
    use crate::agent::hierarchy::task::ExecutionStrategy;

    #[test]
    fn promote_to_hierarchical_for_debug_or_build_intent() {
        let mut out = RouterOutput {
            mode: AgentMode::Single,
            max_iterations: 5,
            max_sub_goals: 3,
            execution_strategy: ExecutionStrategy::Sequential,
            reasoning: Some("简单任务".to_string()),
            routing_strategy: RoutingStrategy::RuleBased,
        };
        let secondary = vec!["execute.run_test_build".to_string()];
        apply_intent_mode_bias(&mut out, Some("execute.debug_diagnose"), &secondary);
        assert_eq!(out.mode, AgentMode::Hierarchical);
        assert_eq!(out.execution_strategy, ExecutionStrategy::Hybrid);
    }

    #[test]
    fn promote_to_multi_agent_for_build_plus_git() {
        let mut out = RouterOutput {
            mode: AgentMode::Hierarchical,
            max_iterations: 30,
            max_sub_goals: 20,
            execution_strategy: ExecutionStrategy::Hybrid,
            reasoning: None,
            routing_strategy: RoutingStrategy::RuleBased,
        };
        let secondary = vec!["execute.run_test_build".to_string()];
        apply_intent_mode_bias(&mut out, Some("execute.git_ops"), &secondary);
        assert_eq!(out.mode, AgentMode::MultiAgent);
        assert_eq!(out.execution_strategy, ExecutionStrategy::Parallel);
    }
}
