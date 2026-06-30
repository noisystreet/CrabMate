//! 同层并行里**单个子目标**的 `tokio` 任务体（自 `execution_parallel` 迁出以降低 `execute_parallel` 的 lizard nloc）。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicBool;

use tokio::sync::Mutex as TokioMutex;
use tokio::sync::mpsc::Receiver;

use crate::agent::hierarchy::{
    artifact_store::ArtifactStore,
    build_state::BuildState,
    execution_helpers::trim_for_detail,
    operator::{OperatorAgent, OperatorConfig, OperatorError},
    subgoal_context,
    subgoal_required_tools::supplement_subgoal_required_tools,
    task::{SubGoal, TaskResult},
    tool_executor::{ProbeCache, ToolExecutor, ToolExecutorContext},
};
use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::sse;
use crate::types::{CommandApprovalDecision, Tool};

/// 并行层级中单个子目标任务的输入（避免 `run_one_parallel_subgoal` 过长参数列表）。
pub(super) struct ParallelSubgoalTask {
    pub goal: SubGoal,
    pub cfg: Arc<AgentConfig>,
    pub llm_backend: Arc<dyn ChatCompletionsBackend>,
    pub client: Arc<reqwest::Client>,
    pub api_key: String,
    pub working_dir: Option<PathBuf>,
    pub tools_defs: Arc<Vec<Tool>>,
    pub build_state: BuildState,
    pub sse_out_operator: Option<tokio::sync::mpsc::Sender<String>>,
    pub sse_out_timeline: Option<tokio::sync::mpsc::Sender<String>>,
    pub tool_approval_out: Option<tokio::sync::mpsc::Sender<String>>,
    pub tool_approval_rx: Option<Arc<TokioMutex<Receiver<CommandApprovalDecision>>>>,
    pub probe_cache: Arc<TokioMutex<ProbeCache>>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub turn_budget: Option<Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
    pub prior: Arc<Vec<TaskResult>>,
    pub pre_snapshot: Arc<ArtifactStore>,
    pub current_ids: Arc<HashSet<String>>,
    pub handler_lookup: crate::tool_registry::HandlerLookupTable,
    pub sync_default_sandbox_backend: Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>,
}

/// 在共享快照上执行单个子目标；供 `JoinSet` 内 `spawn` 使用。
pub(in crate::agent::hierarchy::execution::execution_impl) async fn run_one_parallel_subgoal(
    task: ParallelSubgoalTask,
) -> (String, Result<TaskResult, OperatorError>) {
    let ParallelSubgoalTask {
        goal,
        cfg,
        llm_backend,
        client,
        api_key,
        working_dir,
        tools_defs,
        build_state,
        sse_out_operator,
        sse_out_timeline,
        tool_approval_out,
        tool_approval_rx,
        probe_cache,
        cancel,
        turn_budget,
        prior,
        pre_snapshot,
        current_ids,
        handler_lookup,
        sync_default_sandbox_backend,
    } = task;
    let goal_id = goal.goal_id.clone();
    let goal_description = goal.description.clone();
    let goal_required_tools = goal.required_tools.clone();

    if let Some(reason) = crate::agent::hierarchy::turn_abort::hierarchical_abort_reason_arc(
        sse_out_timeline.as_ref(),
        cancel.as_ref(),
    ) {
        return (
            goal_id,
            Err(OperatorError::ExecutionError(reason.user_message())),
        );
    }

    // 独立子 store，先合并**执行本层前**的共享产物，再运行。
    // 同层并行时：各子目标**彼此**的当轮产物**不可见**（DAG 同层应无互依；若需先后请用边或改顺序策略）。
    let mut store = ArtifactStore::new();
    store.merge_from(&pre_snapshot);
    log::debug!(
        target: "crabmate",
        "[HIERARCHICAL] parallel subgoal {}: merged pre-level artifact snapshot ({} entries; peer goals in this level are not visible to each other)",
        goal_id,
        pre_snapshot.all().len(),
    );

    let mut goal = subgoal_context::ensure_consumes_from_dependencies(
        &goal,
        prior.as_slice(),
        current_ids.as_ref(),
        true,
    );
    if let Some(msg) = subgoal_context::validate_depends_consumes_consistency(&goal) {
        log::warn!(target: "crabmate", "[HIERARCHICAL] I/O: {}", msg);
    }
    subgoal_context::normalize_subgoal_io_contracts(&mut goal);

    let mut allowed_tools = goal.required_tools.clone();
    supplement_subgoal_required_tools(&goal.description, &mut allowed_tools);
    let tools_defs_for_llm = if allowed_tools.is_empty() {
        tools_defs.as_ref().clone()
    } else {
        tools_defs
            .iter()
            .filter(|t| allowed_tools.contains(&t.function.name))
            .cloned()
            .collect()
    };

    let operator_config = OperatorConfig {
        policy: crate::agent::hierarchy::operator::OperatorPolicy {
            max_iterations: 15,
            allowed_tools: allowed_tools.clone(),
            tools_defs: tools_defs_for_llm,
            enable_compile_error_recovery: true,
            compile_error_max_retries: 3,
            enable_dynamic_decomposition: true,
            dynamic_decomposition_threshold: 40,
        },
        runtime: crate::agent::hierarchy::operator::OperatorRuntimeHandles {
            sse_out: sse_out_operator,
            artifact_store: Some(store.clone()),
            build_state: Some(Arc::new(StdMutex::new(build_state.clone()))),
            cancel,
            turn_budget,
        },
    };

    let operator = OperatorAgent::new(operator_config);

    let mut tool_executor_ctx =
        ToolExecutorContext::new(cfg.clone(), working_dir.clone().unwrap_or_default())
            .with_dispatch_handles(handler_lookup, sync_default_sandbox_backend);
    tool_executor_ctx = tool_executor_ctx.with_probe_cache(probe_cache);
    if let Some(ref sse_out_tx) = sse_out_timeline {
        let title = format!("子目标 `{goal_id}`");
        let mut detail = format!(
            "- 阶段：开始执行\n- 目标：{}",
            trim_for_detail(&goal_description, 180)
        );
        if !goal_required_tools.is_empty() {
            detail.push_str("\n- 计划工具：");
            detail.push_str(&goal_required_tools.join(", "));
        }
        let payload = sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "hierarchical_subgoal_started".to_string(),
                title,
                detail: Some(detail),
            },
        });
        let _ = sse::send_string_logged(
            sse_out_tx,
            payload,
            "hierarchical::parallel_subgoal_started_timeline",
        )
        .await;
    }

    if let (Some(out_tx), Some(approval_rx)) = (tool_approval_out, tool_approval_rx) {
        tool_executor_ctx = tool_executor_ctx.with_web_approval_arc(out_tx, approval_rx);
    }

    let tool_executor = ToolExecutor::new(tool_executor_ctx);

    let raw_arts = subgoal_context::collect_artifacts_for_goals(&store, &goal.depends_on);
    let fdeps: Vec<_> = subgoal_context::filter_dependencies_for_injection(&goal, &raw_arts);
    if fdeps.len() < raw_arts.len() {
        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Parallel: dependency filtered {} -> {} for goal_id={}",
            raw_arts.len(),
            fdeps.len(),
            goal.goal_id
        );
    }
    let extra = subgoal_context::build_injected_subgoal_user_extra(&goal, &fdeps, prior.as_slice());
    let result = operator
        .execute_with_tools(
            &goal,
            cfg.as_ref(),
            llm_backend.as_ref(),
            client.as_ref(),
            api_key.as_str(),
            &tool_executor,
            extra.as_deref(),
        )
        .await;

    (goal_id, result)
}
