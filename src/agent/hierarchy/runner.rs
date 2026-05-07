//! 分层 Agent 运行器
//!
//! 提供高层入口，封装 Router → Manager → Operator → Executor 流程。
//!
//! SmartRouter 与可选意图偏置之后的**聚合状态**见 [`HierarchyRoutingResolution`]；流水线步骤标签见 [`HierarchyRunnerPhase`]（与 `tracing` 字段 `hierarchy_runner_phase` 对齐）。

use std::sync::Arc;
use tokio::sync::{
    Mutex,
    mpsc::{Receiver, Sender},
};

use crate::agent::agent_turn::filter_tool_defs_for_executor_kind;
use crate::agent::intent_router::qa_readonly_style_primary;
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::sse;
use crate::types::CommandApprovalDecision;
use tracing::info;

use super::events;
use super::execution::{ExecutionError, HierarchicalExecutionResult};
use super::manager::ManagerAgent;
use super::router::{RouterOutput, SmartRouter};
use super::{AgentMode, ExecutionStrategy, HierarchicalExecutor, ManagerConfig};

/// [`run_hierarchical`] 在 **SmartRouter** 给出 `AgentMode` 之后的显式分支（消除仅靠嵌套 `if` 表达的路径）。
///
/// 与 [`AgentMode`] 对齐：`Hierarchical` / `MultiAgent` 走完整「Manager 分解 → 子目标执行」；
/// `Single` / `ReAct` 走单 Manager + Executor（历史上称 simple fallback）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HierarchyRunnerRoute {
    /// `AgentMode::Single` / `ReAct`：仍调用 Manager 分解，但不按分层路由发射完整流水线 SSE 序。
    SimpleFallback,
    /// `AgentMode::Hierarchical` | `MultiAgent`：`manager_started` SSE → 分解 → 规划 Timeline → 子目标执行。
    FullDecomposedExecution,
}

impl HierarchyRunnerRoute {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SimpleFallback => "simple_fallback",
            Self::FullDecomposedExecution => "full_decomposed_execution",
        }
    }

    pub(crate) fn from_agent_mode(mode: AgentMode) -> Self {
        match mode {
            AgentMode::Hierarchical | AgentMode::MultiAgent => Self::FullDecomposedExecution,
            AgentMode::Single | AgentMode::ReAct => Self::SimpleFallback,
        }
    }
}

/// SmartRouter 完成且（可选）意图偏置应用后的**结构化解析结果**，用于日志字段与后续分支，避免散落布尔与重复读 `RouterOutput`。
#[derive(Debug, Clone)]
pub(crate) struct HierarchyRoutingResolution {
    pub(crate) runner_route: HierarchyRunnerRoute,
    pub(crate) router_output: RouterOutput,
    /// `intent_mode_bias_enabled` 为 true 且意图标签实际改写了 `router_output`。
    pub(crate) intent_bias_modified_router: bool,
}

impl HierarchyRoutingResolution {
    pub(crate) fn resolve_after_smart_router(
        mut router_output: RouterOutput,
        intent_mode_bias_enabled: bool,
        primary_intent: Option<&str>,
        secondary_intents: &[String],
    ) -> Self {
        let intent_bias_modified_router = if intent_mode_bias_enabled {
            apply_intent_mode_bias(&mut router_output, primary_intent, secondary_intents)
        } else {
            false
        };
        let runner_route = HierarchyRunnerRoute::from_agent_mode(router_output.mode);
        Self {
            runner_route,
            router_output,
            intent_bias_modified_router,
        }
    }
}

/// 分层 runner 内离散步骤（与结构化日志 `hierarchy_runner_phase` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HierarchyRunnerPhase {
    RoutingResolved,
    SimpleFallbackEnter,
    ManagerSseStarted,
    ManagerDecomposed,
    SubgoalExecution,
}

impl HierarchyRunnerPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RoutingResolved => "routing_resolved",
            Self::SimpleFallbackEnter => "simple_fallback_enter",
            Self::ManagerSseStarted => "manager_sse_started",
            Self::ManagerDecomposed => "manager_decomposed",
            Self::SubgoalExecution => "subgoal_execution",
        }
    }
}

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
    /// 与主 Agent 轮次共用的进程句柄（工具分发表、Docker 沙盒后端等）。
    pub process_handles: std::sync::Arc<crate::process_handles::ProcessHandles>,
    /// CLI/TUI：镜像 SSE 控制面（分层 Runner 顶部 ThinkingTrace / TimelineLog）。
    pub sse_control_mirror: Option<crate::sse::SseControlMirror>,
}

/// 分层 Agent 运行结果
#[derive(Debug)]
pub struct HierarchyRunnerResult {
    /// 执行结果
    pub execution_result: HierarchicalExecutionResult,
    /// 使用的 Agent 模式
    pub mode: AgentMode,
}

/// `run_simple_fallback` 的输入（路由降级路径与 `HierarchyRunnerParams` 对齐字段）。
struct SimpleFallbackParams<'a> {
    task: &'a str,
    cfg: &'a AgentConfig,
    llm_backend: &'a dyn ChatCompletionsBackend,
    client: std::sync::Arc<reqwest::Client>,
    api_key: String,
    working_dir: std::path::PathBuf,
    sse_out: Option<Sender<String>>,
    tools_defs: &'a [crate::types::Tool],
    tool_approval_out: Option<Sender<String>>,
    tool_approval_rx: Option<Arc<Mutex<Receiver<CommandApprovalDecision>>>>,
    process_handles: std::sync::Arc<crate::process_handles::ProcessHandles>,
    sse_control_mirror: Option<crate::sse::SseControlMirror>,
}

/// `run_full_decomposed_hierarchy` 的输入（避免长参数列表；字段生命周期与一次 runner 调用绑定）。
struct FullDecomposedHierarchyCtx<'a> {
    task: &'a str,
    cfg: &'a AgentConfig,
    llm_backend: &'a dyn ChatCompletionsBackend,
    client: std::sync::Arc<reqwest::Client>,
    api_key: String,
    working_dir: std::path::PathBuf,
    sse_out: Option<Sender<String>>,
    tools_slice: &'a [crate::types::Tool],
    tool_approval_out: Option<Sender<String>>,
    tool_approval_rx: Option<Arc<Mutex<Receiver<CommandApprovalDecision>>>>,
    router_output: RouterOutput,
    process_handles: std::sync::Arc<crate::process_handles::ProcessHandles>,
    sse_control_mirror: Option<crate::sse::SseControlMirror>,
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
        process_handles,
        sse_control_mirror,
    } = params;

    let tools_eff: std::borrow::Cow<'_, [crate::types::Tool]> = if primary_intent
        .as_deref()
        .is_some_and(qa_readonly_style_primary)
    {
        std::borrow::Cow::Owned(filter_tool_defs_for_executor_kind(
            tools_defs,
            cfg,
            PlanStepExecutorKind::ReviewReadonly,
        ))
    } else {
        std::borrow::Cow::Borrowed(tools_defs)
    };
    let tools_slice: &[crate::types::Tool] = tools_eff.as_ref();

    // 1. 智能路由决策
    // 默认使用规则路由，可以通过配置启用 LLM 智能路由
    let use_llm_routing = cfg.hierarchy_routing.enable_llm_routing.unwrap_or(false);
    let router = SmartRouter::new();
    let router_output = router
        .route_smart(
            task,
            cfg,
            llm_backend,
            client.as_ref(),
            &api_key,
            use_llm_routing,
        )
        .await;

    let resolved = HierarchyRoutingResolution::resolve_after_smart_router(
        router_output,
        intent_mode_bias_enabled,
        primary_intent.as_deref(),
        &secondary_intents,
    );
    let HierarchyRoutingResolution {
        runner_route,
        router_output,
        intent_bias_modified_router,
    } = resolved;

    info!(
        target: "crabmate::hierarchy",
        hierarchy_runner_route = runner_route.as_str(),
        hierarchy_runner_phase = HierarchyRunnerPhase::RoutingResolved.as_str(),
        router_mode = router_output.mode.as_str(),
        routing_strategy = ?router_output.routing_strategy,
        max_sub_goals = router_output.max_sub_goals,
        max_iterations = router_output.max_iterations,
        intent_mode_bias_enabled,
        intent_bias_modified_router,
        task_preview = %truncate_string(task, 80),
        "hierarchy runner routed after smart_router"
    );

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

    match runner_route {
        HierarchyRunnerRoute::SimpleFallback => {
            log::info!(
                target: "crabmate",
                "Task complexity {} doesn't require hierarchical execution, falling back",
                router_output.mode.as_str()
            );
            return run_simple_fallback(SimpleFallbackParams {
                task,
                cfg,
                llm_backend,
                client,
                api_key,
                working_dir,
                sse_out,
                tools_defs: tools_slice,
                tool_approval_out,
                tool_approval_rx,
                process_handles: std::sync::Arc::clone(&process_handles),
                sse_control_mirror: sse_control_mirror.clone(),
            })
            .await;
        }
        HierarchyRunnerRoute::FullDecomposedExecution => {}
    }

    run_full_decomposed_hierarchy(FullDecomposedHierarchyCtx {
        task,
        cfg,
        llm_backend,
        client,
        api_key,
        working_dir,
        sse_out,
        tools_slice,
        tool_approval_out,
        tool_approval_rx,
        router_output,
        process_handles: std::sync::Arc::clone(&process_handles),
        sse_control_mirror,
    })
    .await
}

/// Router 判定为分层/多 Agent：发射 Manager 开始 SSE → LLM 分解 → 规划 Timeline → 子目标执行。
async fn run_full_decomposed_hierarchy(
    ctx: FullDecomposedHierarchyCtx<'_>,
) -> Result<HierarchyRunnerResult, ExecutionError> {
    let FullDecomposedHierarchyCtx {
        task,
        cfg,
        llm_backend,
        client,
        api_key,
        working_dir,
        sse_out,
        tools_slice,
        tool_approval_out,
        tool_approval_rx,
        router_output,
        process_handles,
        sse_control_mirror,
    } = ctx;

    info!(
        target: "crabmate::hierarchy",
        hierarchy_runner_route = HierarchyRunnerRoute::FullDecomposedExecution.as_str(),
        router_mode = router_output.mode.as_str(),
        hierarchy_runner_phase = HierarchyRunnerPhase::ManagerSseStarted.as_str(),
        "hierarchy runner full pipeline"
    );

    // 发射 SSE 事件：Manager 开始
    log::info!(target: "crabmate", "[HIERARCHICAL] run_hierarchical: sse_out is {:?}", sse_out.is_some());
    {
        let trace = events::build_manager_started_trace(task);
        let _ = sse::send_sse_control_payload_optional(
            sse_out.as_ref(),
            sse_control_mirror.as_ref(),
            sse::SsePayload::ThinkingTrace { trace },
            "hierarchical::manager_started",
        )
        .await;
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
            tools_slice,
        )
        .await
        .map_err(|e| ExecutionError::MaxFailuresReached(e.to_string()))?;

    info!(
        target: "crabmate::hierarchy",
        hierarchy_runner_route = HierarchyRunnerRoute::FullDecomposedExecution.as_str(),
        router_mode = router_output.mode.as_str(),
        hierarchy_runner_phase = HierarchyRunnerPhase::ManagerDecomposed.as_str(),
        sub_goal_count = manager_output.sub_goals.len(),
        execution_strategy = manager_output.execution_strategy.as_str(),
        "hierarchy runner manager decomposed"
    );

    log::info!(
        target: "crabmate",
        "Manager decomposed task into {} sub-goals, strategy={:?}",
        manager_output.sub_goals.len(),
        manager_output.execution_strategy
    );

    // 发射 SSE 事件顺序：TimelineLog(Manager规划) → ThinkingTrace(manager_finished)
    {
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
        let _ = sse::send_sse_control_payload_optional(
            sse_out.as_ref(),
            sse_control_mirror.as_ref(),
            timeline_payload,
            "hierarchical::manager_plan_timeline",
        )
        .await;

        // ThinkingTrace(manager_finished)
        let trace = events::build_manager_finished_trace(
            manager_output.sub_goals.len(),
            manager_output.execution_strategy.as_str(),
        );
        let _ = sse::send_sse_control_payload_optional(
            sse_out.as_ref(),
            sse_control_mirror.as_ref(),
            sse::SsePayload::ThinkingTrace { trace },
            "hierarchical::manager_finished",
        )
        .await;
    }

    // 3. 执行子目标（传递完整上下文）
    info!(
        target: "crabmate::hierarchy",
        hierarchy_runner_route = HierarchyRunnerRoute::FullDecomposedExecution.as_str(),
        router_mode = router_output.mode.as_str(),
        hierarchy_runner_phase = HierarchyRunnerPhase::SubgoalExecution.as_str(),
        max_operator_iterations = router_output.max_iterations,
        "hierarchy runner executing subgoals"
    );

    let mut executor = HierarchicalExecutor::new(router_output.max_iterations, 3)
        .with_context(
            llm_backend,
            cfg.clone(),
            client.clone(),
            api_key.clone(),
            working_dir.clone(),
        )
        .with_process_tool_handles(
            process_handles.handler_lookup.clone(),
            Arc::clone(&process_handles.sync_default_sandbox_backend),
        )
        .with_tools_defs(tools_slice.to_vec())
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

/// 按意图标签上调路由档位；若改写了 `router_output` 则返回 `true`（供 `intent_bias_modified_router` 日志）。
fn apply_intent_mode_bias(
    router_output: &mut super::router::RouterOutput,
    primary_intent: Option<&str>,
    secondary_intents: &[String],
) -> bool {
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
        return true;
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
        return true;
    }
    false
}

/// 简单降级执行（不进行任务分解）
async fn run_simple_fallback(
    params: SimpleFallbackParams<'_>,
) -> Result<HierarchyRunnerResult, ExecutionError> {
    let SimpleFallbackParams {
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
        process_handles,
        sse_control_mirror,
    } = params;

    info!(
        target: "crabmate::hierarchy",
        hierarchy_runner_route = HierarchyRunnerRoute::SimpleFallback.as_str(),
        hierarchy_runner_phase = HierarchyRunnerPhase::SimpleFallbackEnter.as_str(),
        task_preview = %truncate_string(task, 80),
        "hierarchy runner simple fallback path"
    );

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
    {
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
        let _ = sse::send_sse_control_payload_optional(
            sse_out.as_ref(),
            sse_control_mirror.as_ref(),
            timeline_payload,
            "hierarchical::manager_plan_timeline",
        )
        .await;

        // 2) Manager 开始（ThinkingTrace）
        let trace = events::build_manager_started_trace(task);
        let _ = sse::send_sse_control_payload_optional(
            sse_out.as_ref(),
            sse_control_mirror.as_ref(),
            sse::SsePayload::ThinkingTrace { trace },
            "hierarchical::manager_started",
        )
        .await;

        // 3) Manager 完成（ThinkingTrace）
        let trace = events::build_manager_finished_trace(
            manager_output.sub_goals.len(),
            manager_output.execution_strategy.as_str(),
        );
        let _ = sse::send_sse_control_payload_optional(
            sse_out.as_ref(),
            sse_control_mirror.as_ref(),
            sse::SsePayload::ThinkingTrace { trace },
            "hierarchical::manager_finished",
        )
        .await;
        // 不在此下发 `assistant_answer_phase`：须等子目标进度或最终汇总，由 execution / handle_execution_result 统一发送，避免「进入终答相却无正文」的错位。
    }

    let mut executor = HierarchicalExecutor::new(10, 3)
        .with_context(llm_backend, cfg.clone(), client, api_key, working_dir)
        .with_process_tool_handles(
            process_handles.handler_lookup.clone(),
            Arc::clone(&process_handles.sync_default_sandbox_backend),
        )
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
    use super::{HierarchyRoutingResolution, HierarchyRunnerPhase, HierarchyRunnerRoute};
    use crate::agent::hierarchy::router::{AgentMode, RouterOutput, RoutingStrategy};
    use crate::agent::hierarchy::task::ExecutionStrategy;

    #[test]
    fn resolve_runner_route_matches_agent_mode() {
        assert_eq!(
            HierarchyRunnerRoute::from_agent_mode(AgentMode::Hierarchical),
            HierarchyRunnerRoute::FullDecomposedExecution
        );
        assert_eq!(
            HierarchyRunnerRoute::from_agent_mode(AgentMode::MultiAgent),
            HierarchyRunnerRoute::FullDecomposedExecution
        );
        assert_eq!(
            HierarchyRunnerRoute::from_agent_mode(AgentMode::Single),
            HierarchyRunnerRoute::SimpleFallback
        );
        assert_eq!(
            HierarchyRunnerRoute::from_agent_mode(AgentMode::ReAct),
            HierarchyRunnerRoute::SimpleFallback
        );
    }

    #[test]
    fn hierarchy_runner_phase_strings_stable() {
        assert_eq!(
            HierarchyRunnerPhase::RoutingResolved.as_str(),
            "routing_resolved"
        );
    }

    #[test]
    fn hierarchy_routing_resolution_bias_disabled_leaves_router() {
        let out = RouterOutput {
            mode: AgentMode::Single,
            max_iterations: 5,
            max_sub_goals: 3,
            execution_strategy: ExecutionStrategy::Sequential,
            reasoning: None,
            routing_strategy: RoutingStrategy::RuleBased,
        };
        let resolved = HierarchyRoutingResolution::resolve_after_smart_router(
            out.clone(),
            false,
            Some("execute.debug_diagnose"),
            &[],
        );
        assert!(!resolved.intent_bias_modified_router);
        assert_eq!(resolved.router_output.mode, out.mode);
        assert_eq!(resolved.runner_route, HierarchyRunnerRoute::SimpleFallback);
    }

    #[test]
    fn hierarchy_routing_resolution_bias_promotes_sets_flag_and_route() {
        let out = RouterOutput {
            mode: AgentMode::Single,
            max_iterations: 5,
            max_sub_goals: 3,
            execution_strategy: ExecutionStrategy::Sequential,
            reasoning: None,
            routing_strategy: RoutingStrategy::RuleBased,
        };
        let resolved = HierarchyRoutingResolution::resolve_after_smart_router(
            out,
            true,
            Some("execute.debug_diagnose"),
            &[],
        );
        assert!(resolved.intent_bias_modified_router);
        assert_eq!(resolved.router_output.mode, AgentMode::Hierarchical);
        assert_eq!(
            resolved.runner_route,
            HierarchyRunnerRoute::FullDecomposedExecution
        );
    }

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
