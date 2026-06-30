//! Operator ReAct 主循环辅助：进度 SSE 上下文、预算门禁（从 `react_loop.rs` 抽出以降低 CCN/nloc）。

use std::time::Instant;

use crate::config::{AgentConfig, TurnBudgetConfig};

use super::super::task::{TaskResult, TaskStatus};
use super::prompt;
use super::state::{ReactState, SubgoalPhase};
use super::types::{OperatorAgent, OperatorPolicy};
use crate::types::{Message, MessageContent};

/// ReAct 进度时间线 SSE 入参（避免 `emit_react_progress_timeline` 形参过多）。
pub(super) struct ReactProgressTimelineCtx<'a> {
    pub goal_id: &'a str,
    pub iteration: usize,
    pub max_iterations: usize,
    pub tool_calls: Option<usize>,
    pub phase: Option<SubgoalPhase>,
    pub error_count: Option<usize>,
    pub stagnant_rounds: Option<usize>,
    pub first_error: Option<&'a str>,
    pub turn_budget_cfg: Option<&'a TurnBudgetConfig>,
}

impl<'a> ReactProgressTimelineCtx<'a> {
    pub fn from_react_state(
        goal_id: &'a str,
        state: &'a ReactState,
        max_iterations: usize,
        convergence_goal: bool,
        turn_budget_cfg: Option<&'a TurnBudgetConfig>,
    ) -> Self {
        Self {
            goal_id,
            iteration: state.iteration,
            max_iterations,
            tool_calls: Some(state.tool_names_chron.len()),
            phase: convergence_goal.then_some(state.phase),
            error_count: if convergence_goal {
                state.progress.last_error_count
            } else {
                None
            },
            stagnant_rounds: convergence_goal.then_some(state.progress.rounds_without_progress),
            first_error: if convergence_goal {
                state.progress.last_first_error_signature.as_deref()
            } else {
                None
            },
            turn_budget_cfg,
        }
    }
}

/// 墙钟 / LLM 次数 / Token 预算门禁；耗尽时返回面向用户的短消息。
pub(super) fn react_loop_budget_limit_message(
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
    cfg: &AgentConfig,
    elapsed_secs: u64,
) -> Option<String> {
    if let Some(budget) = turn_budget {
        if let Err(msg) = budget.deny_llm_call_if_exhausted(&cfg.turn_budget) {
            return Some(msg);
        }
        if budget.wall_clock_exceeded(&cfg.turn_budget) {
            return Some(
                crate::agent::turn_budget::turn_wall_clock_limit_user_message(
                    cfg.turn_budget.max_turn_duration_seconds,
                ),
            );
        }
        return None;
    }
    if crate::agent::turn_budget::turn_wall_clock_exceeded(
        cfg.turn_budget.max_turn_duration_seconds,
        elapsed_secs,
    ) {
        Some(
            crate::agent::turn_budget::turn_wall_clock_limit_user_message(
                cfg.turn_budget.max_turn_duration_seconds,
            ),
        )
    } else {
        None
    }
}

/// 构建 ReAct 初始 `ReactState` 与 system/user 消息。
pub(super) fn init_react_state_for_goal(
    policy: &OperatorPolicy,
    goal: &super::super::task::SubGoal,
    enhanced_context: Option<String>,
) -> ReactState {
    let mut state = ReactState {
        iteration: 0,
        messages: Vec::new(),
        observations: Vec::new(),
        task_completed: false,
        completion_reason: None,
        current_working_dir: None,
        consecutive_failures: 0,
        last_failed_tool: None,
        last_error_type: None,
        recent_commands: Vec::new(),
        duplicate_command_count: 0,
        lightweight_command_cache: std::collections::HashMap::new(),
        tools_used: std::collections::HashSet::new(),
        tool_names_chron: Vec::new(),
        dynamic_decomposition_count: 0,
        phase: SubgoalPhase::Diagnose,
        progress: super::state::ConvergenceProgress::default(),
        last_reported_phase: None,
        attempted_compile_configs: Vec::new(),
    };
    let system_prompt =
        prompt::build_system_prompt(policy, goal, state.current_working_dir.as_deref());
    state.messages.push(Message {
        role: "system".to_string(),
        content: Some(MessageContent::Text(system_prompt)),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    });
    let task_description = enhanced_context
        .map(|ctx| format!("{}\n\n{}", goal.description, ctx))
        .unwrap_or_else(|| goal.description.clone());
    let user_task = format!(
        "任务：{}\n\n请执行任务并通过工具调用完成任务。",
        task_description
    );
    state.messages.push(Message {
        role: "user".to_string(),
        content: Some(MessageContent::Text(user_task)),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    });
    state
}

impl OperatorAgent {
    /// 预算耗尽时返回 `Some(TaskResult)`，否则 `None`。
    pub(super) fn react_loop_budget_exhausted_result(
        &self,
        goal_id: &str,
        state: &ReactState,
        cfg: &AgentConfig,
        start_time: Instant,
    ) -> Option<TaskResult> {
        let msg = react_loop_budget_limit_message(
            self.config.runtime.turn_budget.as_ref(),
            cfg,
            start_time.elapsed().as_secs(),
        )?;
        tracing::warn!(
            target: "crabmate::hierarchy",
            limiter = "turn_budget",
            goal_id = %goal_id,
            "Operator ReAct: turn budget exhausted before LLM call"
        );
        Some(self.task_result_on_turn_budget_exhausted(goal_id, state, msg, start_time))
    }

    pub(super) fn react_loop_max_iterations_result(
        &self,
        goal_id: &str,
        state: &ReactState,
        start_time: Instant,
    ) -> TaskResult {
        TaskResult {
            task_id: goal_id.to_string(),
            status: TaskStatus::Failed {
                reason: "Max iterations reached".to_string(),
            },
            output: Some(self.build_output_summary(state)),
            error: Some("Max iterations reached".to_string()),
            artifacts: Vec::new(),
            duration_ms: start_time.elapsed().as_millis() as u64,
            tools_invoked: state.tool_names_chron.clone(),
        }
    }
}
