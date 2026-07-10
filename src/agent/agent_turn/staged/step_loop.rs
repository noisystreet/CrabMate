//! 分阶段规划 **步执行循环**（`StepRunning` 相位）的纯决策与 reduce 表驱动逻辑。
//!
//! 收拢原 **`staged_step_fsm`**、**`step_loop_fsm`**、**`step_iteration_fsm`**、
//! **`steps_loop_route_fsm`**、**`step_iteration_reduce`**、**`steps_loop_reduce`**、
//! **`step_patch_route_fsm`**、**`step_patch_recover_reduce`**。
//!
//! **不**发起 LLM / outer_loop / SSE；副作用留在 **`steps_loop`** 等 IO driver。
//! 见 `docs/design/per_state_machine_consolidation.md` §3.2（`StepRunning` 相位）。

use std::collections::HashMap;

use crate::agent::agent_turn::errors::RunAgentTurnError;
use crate::agent::plan_artifact::{PlanStepAcceptance, PlanStepControlFlow, PlanStepV1};
use crate::agent::step_executor_policy::executor_kind_user_label;
use crate::config::StagedPlanFeedbackMode;
use crate::types::Message;
use crabmate_display_rules::STAGED_FSM_CONTROL_FLOW_PREFIX;

use super::StagedPlanRunOutcome;
use super::empty_execution::{
    staged_step_empty_execution_is_reason, staged_step_empty_execution_patch_detail,
};

// --- Patch budget / patch planner gate (原 staged_step_fsm) ---

/// 单步失败后触发补丁规划员的尝试次数上限（与 `run_staged_plan_steps_loop` 中原 `for _ in 0..max_retries` 一致）。
#[inline]
pub(crate) fn staged_patch_budget_after_step_failure(
    step_max_retries: Option<u32>,
    cfg_staged_plan_patch_max_attempts: usize,
) -> usize {
    step_max_retries.unwrap_or(cfg_staged_plan_patch_max_attempts as u32) as usize
}

/// 「工具未全部成功」分支的补丁尝试上限（仅使用全局配置计数）。
#[inline]
pub(crate) fn staged_patch_budget_tool_messages_not_ok(
    cfg_staged_plan_patch_max_attempts: usize,
) -> usize {
    cfg_staged_plan_patch_max_attempts
}

/// 是否启用补丁规划员路径（与 `staged_plan_feedback_mode == PatchPlanner` 等价）。
#[inline]
pub(crate) fn staged_step_patch_planner_enabled(mode: StagedPlanFeedbackMode) -> bool {
    matches!(mode, StagedPlanFeedbackMode::PatchPlanner)
}

// --- Transition jump & injected user body (原 step_loop_fsm) ---

/// 与 `mod.rs` 原 `compute_transition_trigger` 等价：匹配一条 transition、遵守 `max_loops`、更新计数器。
pub(crate) fn staged_step_transition_trigger(
    step: &PlanStepV1,
    run_failed_or_verify_failed: bool,
    step_verify_failed_reason: &Option<String>,
    transition_counters: &mut HashMap<String, u32>,
) -> Option<(String, String)> {
    let transitions = step.transitions.as_ref()?;
    let target = select_transition_rule(transitions, run_failed_or_verify_failed)?;
    let key = format!("{}->{}", step.id, target.target_step_id);
    let count = transition_counters.entry(key).or_insert(0);
    if *count >= target.max_loops.unwrap_or(3) {
        return None;
    }
    *count += 1;
    let reason = if run_failed_or_verify_failed {
        step_verify_failed_reason
            .clone()
            .unwrap_or_else(|| "执行错误".to_string())
    } else {
        "执行成功".to_string()
    };
    Some((target.target_step_id.clone(), reason))
}

fn select_transition_rule(
    transitions: &[PlanStepControlFlow],
    run_failed_or_verify_failed: bool,
) -> Option<&PlanStepControlFlow> {
    if run_failed_or_verify_failed {
        transitions
            .iter()
            .find(|t| t.condition == "on_verify_fail" || t.condition == "always")
    } else {
        transitions
            .iter()
            .find(|t| t.condition == "on_verify_success" || t.condition == "always")
    }
}

/// 若 `target_step_id` 落在 **`original_steps`** 中：截断当前队列至 `i+1`，追加从目标起的后缀（id 加 `-loop{i}`），返回用户可见反馈正文与 SSE 状态。
pub(crate) fn try_apply_staged_plan_control_flow_jump(
    step: &PlanStepV1,
    i: usize,
    plan_steps: &mut Vec<PlanStepV1>,
    original_steps: &[PlanStepV1],
    transition_counters: &mut HashMap<String, u32>,
    run_failed_or_verify_failed: bool,
    step_verify_failed_reason: &Option<String>,
) -> Option<(String, &'static str)> {
    let (target_id, reason) = staged_step_transition_trigger(
        step,
        run_failed_or_verify_failed,
        step_verify_failed_reason,
        transition_counters,
    )?;
    let target_idx = original_steps.iter().position(|s| s.id == target_id)?;
    let mut new_suffix = original_steps[target_idx..].to_vec();
    let loop_suffix = format!("-loop{i}");
    for s in &mut new_suffix {
        s.id = format!("{}{}", s.id, loop_suffix);
    }
    plan_steps.truncate(i.saturating_add(1));
    plan_steps.extend(new_suffix);
    let fb = format!(
        "{prefix}：触发控制流跳转\n\
         根据规划设定的 transitions 规则，由于 [{reason}]，系统已追加回退或跳转到步骤 `{target_id}` 的执行指令。\n\
         请注意调整接下来的工具调用。",
        prefix = STAGED_FSM_CONTROL_FLOW_PREFIX,
        reason = reason,
        target_id = target_id
    );
    let sse_status = if run_failed_or_verify_failed {
        "failed"
    } else {
        "ok"
    };
    Some((fb, sse_status))
}

/// 注入执行器的单步 **user** 正文（与 `run_staged_plan_steps_loop` 内 `format!` 对齐）。
///
/// `immutable_user_goal`：系统持有的本轮用户原文（不变层）；有则置于分步说明之前以减少步内漂移。
pub(crate) fn staged_injected_step_user_body(
    step_index: usize,
    n: usize,
    step: &PlanStepV1,
    immutable_user_goal: Option<&str>,
) -> String {
    let immutable_prefix = immutable_user_goal
        .filter(|g| !g.trim().is_empty())
        .map(crate::agent::plan_optimizer::staged_rolling_immutable_step_user_prefix)
        .unwrap_or_default();
    let summary_hint = if step_index == n && n > 1 {
        format!(
            "\n本步为最后一步，终答中请简要列出本轮全部 {} 个步骤的完成情况（可对每步附简短说明）。",
            n
        )
    } else {
        String::new()
    };
    let sub_agent_hint = match step.executor_kind {
        Some(k) => format!(
            "\n- **子代理角色**（本步 `tools` 已按策略表收窄）：`{}` — {}\n",
            k.as_snake_case_str(),
            executor_kind_user_label(k)
        ),
        None => String::new(),
    };
    format!(
        "{immutable_prefix}### 分步 {}/{}\n{}{}{}\n- id: {}\n- 描述: {}",
        step_index,
        n,
        crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE,
        summary_hint,
        sub_agent_hint,
        step.id,
        step.description
    )
}

// --- Step iteration after outer loop (原 step_iteration_fsm) ---

/// 单步执行器内子阶段（对应设计稿 **`StepRunning.sub`**：`BeforeStepLlm` / `InOuterLoop` / 失败处理子集）。
/// 实现上由 **`staged/mod.rs`** 的 **`staged_step_run_outer_half`** / **`staged_step_run_after_outer_half`** 对应；本类型为**词汇表**（检索/文档对齐），生产路径不直接分支于该枚举。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedStepRunningSub {
    /// `step_started` 起至注入本步 user、设置 `turn_planner_hints.step_executor_constraint` 止（尚未 `run_agent_outer_loop`）。
    BeforeStepLlm,
    /// `run_agent_outer_loop` 与可选 acceptance 验证。
    InOuterLoop,
    /// outer 返回之后：transition、执行/验收失败补丁、取消、工具消息检查与补丁、或成功 SSE（设计稿中的 *AfterStepFailure* 为该阶段内子路径）。
    AfterOuterLoop,
}

/// `try_apply_staged_plan_control_flow_jump` 未触发时，根据 outer_loop 与验收结果划分阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StagedStepAfterOuterLoop {
    /// 执行与验收均成功，进入「本步 user 之后 tool 消息是否均 ok」的检查。
    ProceedToToolCheck,
    /// 执行失败或验收失败；由调用方跑补丁循环或早退。
    ExecutionOrVerifyFailed {
        outer_loop_error: Option<String>,
        verify_failure_reason: Option<String>,
    },
}

pub(crate) fn staged_step_after_outer_loop(
    run_step: &Result<(), RunAgentTurnError>,
    step_verify_failed_reason: &Option<String>,
) -> StagedStepAfterOuterLoop {
    if let Err(e) = run_step {
        return StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
            outer_loop_error: Some(e.to_string()),
            verify_failure_reason: None,
        };
    }
    if let Some(r) = step_verify_failed_reason {
        return StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
            outer_loop_error: None,
            verify_failure_reason: Some(r.clone()),
        };
    }
    StagedStepAfterOuterLoop::ProceedToToolCheck
}

/// 失败路径上补丁耗尽时构造 `StepRetryExhausted` 文案（与历史 `run_staged_plan_steps_loop` 一致）。
pub(crate) fn staged_step_failure_retry_exhausted_message(
    run_step: &Result<(), RunAgentTurnError>,
    step_verify_failed_reason: &Option<String>,
) -> String {
    if let Err(e) = run_step {
        return e.to_string();
    }
    step_verify_failed_reason
        .clone()
        .unwrap_or_else(|| "局部修复耗尽上限".to_string())
}

/// 工具消息检查阶段：是否进入「工具未全部成功」的补丁尝试循环。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedStepToolPhaseRoute {
    /// 发送本步 `ok` 并推进（含 `tools_ok==false` 且未启用 patch planner 时沿用既有语义）。
    EmitStepSuccess,
    /// `tools_ok==false` 且启用 patch planner：由调用方跑补丁循环，可能 `continue` 同一步。
    AttemptToolFailurePatches,
}

/// 单次 `run_staged_plan_steps_loop` 迭代结束方式（不含墙钟：由外层检查）。
pub(crate) enum StagedStepIterationCtl {
    /// 补丁重规划后重试当前下标（`i` 不变）。
    RetryCurrentStep { n: usize },
    /// 本步已完结（transition 或成功），调用方将 `i += 1`。
    AdvanceToNextStep { n: usize, completed_steps: usize },
    /// 本步成功后检测到取消（与历史：先发 `step_finished(cancelled)` 再 `break`）。
    CancelledAfterOuterOk,
}

pub(crate) fn staged_step_tool_phase_route(
    tools_ok: bool,
    patch_planner_enabled: bool,
) -> StagedStepToolPhaseRoute {
    if tools_ok {
        StagedStepToolPhaseRoute::EmitStepSuccess
    } else if patch_planner_enabled {
        StagedStepToolPhaseRoute::AttemptToolFailurePatches
    } else {
        StagedStepToolPhaseRoute::EmitStepSuccess
    }
}

/// 与 [`crate::agent::turn_budget::turn_wall_clock_exceeded`] 一致：`max_turn_duration_seconds == 0` 表示不限制。
pub(crate) fn staged_step_wall_clock_exceeded(
    max_turn_duration_seconds: u64,
    elapsed_secs: u64,
) -> bool {
    crate::agent::turn_budget::turn_wall_clock_exceeded(max_turn_duration_seconds, elapsed_secs)
}

pub(crate) fn staged_step_verify_fail_patch_detail(
    verify_reason: &str,
    acceptance_ref: Option<&PlanStepAcceptance>,
) -> String {
    let reference_line = acceptance_ref
        .and_then(|a| a.compact_reference_for_planner_feedback())
        .map(|line| format!("- **参考验收（acceptance，r）**：{line}\n"))
        .unwrap_or_default();
    format!(
        "### 偏差结构化（验证失败）\n\
         {reference_line}\
         - **观测 / 偏差（step_verifier）**：{verify_reason}\n\
         若 `观测` 行以 `exit_code_mismatch:`、`stdout_missing:`、`stderr_missing:`、`combined_output_missing:`、`file_not_found:`、`json_path_mismatch:` 等键开头，请对症调整命令、工具选择或 `acceptance` 锚点。\n\
         请根据对话历史缩短或调整后续步骤，并在补丁中修复此问题。"
    )
}

/// 本分步内未全部成功的 `role: tool` 摘要，供补丁规划 **user** 的「观测 y」段落。
pub(crate) fn staged_step_tool_failure_patch_detail(
    messages: &[Message],
    step_user_index: usize,
    acceptance_ref: Option<&PlanStepAcceptance>,
) -> String {
    const PREVIEW_CHARS: usize = 240;
    const MAX_TOOL_LINES: usize = 6;

    let reference_line = acceptance_ref
        .and_then(|a| a.compact_reference_for_planner_feedback())
        .map(|line| format!("- **参考验收（acceptance，r）**：{line}\n"))
        .unwrap_or_default();

    if step_user_index >= messages.len() {
        return format!(
            "### 偏差结构化（工具未全部成功）\n\
             {reference_line}\
             - **观测**：`step_user_index` 越界；请直接阅读对话历史中本分步内的 `role: tool`。\n\
             {STAGED_STEP_TOOL_MSG_FAIL_DETAIL}"
        );
    }

    let mut lines: Vec<String> = Vec::new();
    let mut saw_repeat_short_circuit = false;
    let end = crate::types::staged_step_window_end_exclusive(messages, step_user_index);
    let mut i = step_user_index.saturating_add(1);
    while i < end {
        let m = &messages[i];
        if m.role == "tool" {
            let name = m.name.as_deref().unwrap_or("");
            let content = crate::types::message_content_as_str(&m.content).unwrap_or("");
            if !crate::tool_result::tool_message_content_ok_for_model(content, name) {
                let tool_error_code = tool_failure_error_code(name, content);
                saw_repeat_short_circuit |=
                    tool_error_code_is_repeat_short_circuit(tool_error_code.as_deref());
                let ec = tool_error_code
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("error_code={s}"))
                    .unwrap_or_else(|| "error_code=(none_or_unparsed)".to_string());
                let preview =
                    crate::redact::preview_chars(content, PREVIEW_CHARS).replace('`', "'");
                lines.push(format!("- **工具 `{name}`**：{ec}；输出摘要：{preview}"));
                if lines.len() >= MAX_TOOL_LINES {
                    lines.push(
                        "- **…**：更多失败工具已省略；请读取完整 `role: tool` 历史。".to_string(),
                    );
                    break;
                }
            }
        }
        i += 1;
    }

    let obs_block = if lines.is_empty() {
        format!(
            "### 偏差结构化（工具未全部成功）\n\
             {reference_line}\
             - **观测**：未解析到具体失败工具条目；请扫本分步内全部 `role: tool`。\n"
        )
    } else {
        format!(
            "### 偏差结构化（工具未全部成功）\n\
             {reference_line}\
             {}\n",
            lines.join("\n")
        )
    };

    format!(
        "{obs_block}\
         {}\
         若 `error_code=` 可对应 `invalid_args` / `timeout` / `not_found` 等，请在补丁中调整工具入参、白名单或前置只读步。\n\
         {STAGED_STEP_TOOL_MSG_FAIL_DETAIL}",
        repeat_short_circuit_patch_rule(saw_repeat_short_circuit)
    )
}

fn tool_failure_error_code(tool_name: &str, content: &str) -> Option<String> {
    crate::tool_result::normalize_tool_message_content(content)
        .and_then(|env| env.error_code)
        .or_else(|| crate::tool_result::parse_legacy_output(tool_name, content).error_code)
}

fn tool_error_code_is_repeat_short_circuit(error_code: Option<&str>) -> bool {
    matches!(
        error_code,
        Some("repeated_tool_failure_short_circuit" | "repeated_tool_family_failure_short_circuit")
    )
}

fn repeat_short_circuit_patch_rule(saw_repeat_short_circuit: bool) -> &'static str {
    if saw_repeat_short_circuit {
        "- **硬约束**：本步已触发重复失败短路；补丁计划不得再次生成相同 `run_command` 或同类命令。必须改为读取配置/解释失败原因/换用不同构建配置或直接向用户报告阻塞原因。\n"
    } else {
        ""
    }
}

/// 执行子循环 `Err` 时写入补丁规划 **user** 的详情（截断，避免撑爆上下文）。
pub(crate) fn staged_step_exec_fail_patch_detail(outer_loop_error: &str) -> String {
    const MAX_ERR_CHARS: usize = 1200;
    let tail = crate::redact::preview_chars(outer_loop_error, MAX_ERR_CHARS);
    format!(
        "{}\n- 执行子循环错误摘要：{}",
        STAGED_STEP_OUTER_LOOP_FAIL_DETAIL, tail
    )
}

pub(crate) const STAGED_STEP_OUTER_LOOP_FAIL_DETAIL: &str =
    "请根据对话历史缩短或调整后续步骤；若属环境/权限问题请在补丁中显式增加修复步。";

pub(crate) const STAGED_STEP_TOOL_MSG_FAIL_DETAIL: &str = "请阅读本步对应的 `role: tool` 输出（含失败原因），修订从当前步起的 `steps`（可替换、拆分或追加一步）。";

// --- Post-outer route table (原 steps_loop_route_fsm) ---

/// transition 已排除后，本步 outer_loop 结果 → 下一步 I/O 形态（表驱动入口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedStepPostOuterRoute {
    /// 执行子循环 `Err` 或步级验收失败 → 补丁恢复或耗尽。
    ExecOrVerifyFailed,
    /// SSE 关闭或用户取消（outer 已成功）。
    Cancelled,
    /// 工具消息未全部成功且启用 patch planner。
    ToolFailurePatch,
    /// 本步成功收尾（含 patch 关闭时 tools 未全 ok 的既有语义）。
    EmitSuccess,
}

crate::impl_as_str!(StagedStepPostOuterRoute, {
    Self::ExecOrVerifyFailed => "exec_or_verify_failed",
    Self::Cancelled => "cancelled",
    Self::ToolFailurePatch => "tool_failure_patch",
    Self::EmitSuccess => "emit_success",
});

/// 由已分类的 **`StagedStepAfterOuterLoop`** 与取消/工具检查输入解析路由。
pub(crate) fn resolve_staged_step_post_outer_route(
    after_outer: StagedStepAfterOuterLoop,
    cancelled: bool,
    tools_ok: bool,
    patch_planner_on: bool,
) -> StagedStepPostOuterRoute {
    if matches!(
        after_outer,
        StagedStepAfterOuterLoop::ExecutionOrVerifyFailed { .. }
    ) {
        return StagedStepPostOuterRoute::ExecOrVerifyFailed;
    }
    if cancelled {
        return StagedStepPostOuterRoute::Cancelled;
    }
    match staged_step_tool_phase_route(tools_ok, patch_planner_on) {
        StagedStepToolPhaseRoute::AttemptToolFailurePatches => {
            StagedStepPostOuterRoute::ToolFailurePatch
        }
        StagedStepToolPhaseRoute::EmitStepSuccess => StagedStepPostOuterRoute::EmitSuccess,
    }
}

/// 组合 outer_loop 结果、验收与 cancel/tools 输入（供 **`steps_loop`** 单点调用）。
pub(crate) fn resolve_staged_step_post_outer_route_from_results(
    run_step: &Result<(), RunAgentTurnError>,
    step_verify_failed_reason: &Option<String>,
    cancelled: bool,
    tools_ok: bool,
    patch_planner_on: bool,
) -> StagedStepPostOuterRoute {
    let after_outer = staged_step_after_outer_loop(run_step, step_verify_failed_reason);
    resolve_staged_step_post_outer_route(after_outer, cancelled, tools_ok, patch_planner_on)
}

// --- Step iteration reduce (原 step_iteration_reduce) ---

/// `resolve_staged_step_post_outer_route*` 之后的纯 reduce 输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepIterationReduceAction {
    ExecOrVerifyFailed,
    Cancelled,
    ToolFailurePatch,
    EmitSuccessAdvance,
}

impl StepIterationReduceAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ExecOrVerifyFailed => "exec_or_verify_failed",
            Self::Cancelled => "cancelled",
            Self::ToolFailurePatch => "tool_failure_patch",
            Self::EmitSuccessAdvance => "emit_success_advance",
        }
    }
}

impl StagedStepPostOuterRoute {
    pub(crate) fn reduce_action(self) -> StepIterationReduceAction {
        match self {
            Self::ExecOrVerifyFailed => StepIterationReduceAction::ExecOrVerifyFailed,
            Self::Cancelled => StepIterationReduceAction::Cancelled,
            Self::ToolFailurePatch => StepIterationReduceAction::ToolFailurePatch,
            Self::EmitSuccess => StepIterationReduceAction::EmitSuccessAdvance,
        }
    }
}

pub(crate) fn reduce_staged_step_post_outer_route(
    route: StagedStepPostOuterRoute,
) -> StepIterationReduceAction {
    route.reduce_action()
}

/// 成功收尾时的迭代控制（与历史 **`StagedStepIterationCtl::AdvanceToNextStep`** 对齐）。
pub(crate) fn step_iteration_ctl_for_emit_success(
    n: usize,
    step_index: usize,
) -> StagedStepIterationCtl {
    StagedStepIterationCtl::AdvanceToNextStep {
        n,
        completed_steps: step_index,
    }
}

// --- Steps queue loop reduce (原 steps_loop_reduce) ---

/// 步队列迭代前守卫（墙钟 / SSE / 取消）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepsLoopPreflightReduceAction {
    Continue,
    BreakCancelled,
}

pub(crate) fn reduce_steps_loop_preflight(
    sse_closed: bool,
    user_cancelled: bool,
) -> StepsLoopPreflightReduceAction {
    if sse_closed || user_cancelled {
        StepsLoopPreflightReduceAction::BreakCancelled
    } else {
        StepsLoopPreflightReduceAction::Continue
    }
}

/// 单次步迭代 **`StagedStepIterationCtl`** → 队列状态更新（无 IO）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepsLoopIterationReduceAction {
    RetryCurrentStep { n: usize },
    AdvanceToNextStep { n: usize, completed_steps: usize },
    BreakCancelled,
}

pub(crate) fn reduce_steps_loop_iteration_ctl(
    ctl: StagedStepIterationCtl,
) -> StepsLoopIterationReduceAction {
    match ctl {
        StagedStepIterationCtl::RetryCurrentStep { n } => {
            StepsLoopIterationReduceAction::RetryCurrentStep { n }
        }
        StagedStepIterationCtl::AdvanceToNextStep { n, completed_steps } => {
            StepsLoopIterationReduceAction::AdvanceToNextStep { n, completed_steps }
        }
        StagedStepIterationCtl::CancelledAfterOuterOk => {
            StepsLoopIterationReduceAction::BreakCancelled
        }
    }
}

pub(crate) fn steps_loop_finish_status(staged_loop_cancelled: bool) -> &'static str {
    if staged_loop_cancelled {
        "cancelled"
    } else {
        "ok"
    }
}

pub(crate) fn should_push_steps_loop_separator(
    n: usize,
    staged_loop_cancelled: bool,
    completed_steps: usize,
) -> bool {
    n == 0 || (staged_loop_cancelled && completed_steps == 0)
}

/// driver 缺席时的默认 outcome（与历史行为一致：继续滚动视界规划）。
pub(crate) fn steps_loop_outcome_without_driver() -> StagedPlanRunOutcome {
    StagedPlanRunOutcome::ContinuePlanning
}

// --- Step patch failure route (原 step_patch_route_fsm) ---

/// 步失败后进入补丁规划员前的失败分类（执行 / 验收 / 工具三条路径统一词汇）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StagedStepPatchFailureKind {
    /// `run_agent_outer_loop` 返回 `Err`。
    OuterLoopError,
    /// `step_verifier` 或空执行检测失败。
    StepVerifyFail {
        reason: String,
        empty_execution: bool,
    },
    /// 本步 `role: tool` 未全部成功。
    ToolMessagesNotOk,
}

crate::impl_as_str!(StagedStepPatchFailureKind, {
    Self::OuterLoopError => "outer_loop_error",
    Self::StepVerifyFail { .. } => "step_verify_fail",
    Self::ToolMessagesNotOk => "tool_messages_not_ok",
});

/// 补丁反馈文案所需的只读上下文（与失败种类组合使用）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct StagedStepPatchFeedbackCtx<'a> {
    pub outer_loop_error_text: Option<&'a str>,
    pub acceptance: Option<&'a PlanStepAcceptance>,
    pub messages: &'a [Message],
    pub step_user_index: usize,
}

/// 由 outer 结果与验收失败原因解析补丁失败种类（无 patch 时由调用方短路）。
pub(crate) fn resolve_staged_step_patch_failure_kind(
    step_verify_failed_reason: &Option<String>,
    has_outer_loop_error: bool,
) -> Option<StagedStepPatchFailureKind> {
    if let Some(vr) = step_verify_failed_reason {
        return Some(StagedStepPatchFailureKind::StepVerifyFail {
            reason: vr.clone(),
            empty_execution: staged_step_empty_execution_is_reason(vr),
        });
    }
    if has_outer_loop_error {
        return Some(StagedStepPatchFailureKind::OuterLoopError);
    }
    None
}

/// 补丁规划 **user** 正文的 `detail` 与 `reason_zh`（表驱动，与历史 `steps_loop` 文案一致）。
pub(crate) fn staged_step_patch_failure_feedback(
    kind: &StagedStepPatchFailureKind,
    ctx: StagedStepPatchFeedbackCtx<'_>,
) -> (String, &'static str) {
    match kind {
        StagedStepPatchFailureKind::OuterLoopError => {
            let detail = ctx
                .outer_loop_error_text
                .map(staged_step_exec_fail_patch_detail)
                .unwrap_or_else(|| STAGED_STEP_OUTER_LOOP_FAIL_DETAIL.to_string());
            (detail, "执行子循环返回错误")
        }
        StagedStepPatchFailureKind::StepVerifyFail {
            reason,
            empty_execution,
        } => {
            let detail = if *empty_execution {
                staged_step_empty_execution_patch_detail(reason, ctx.acceptance)
            } else {
                staged_step_verify_fail_patch_detail(reason, ctx.acceptance)
            };
            (detail, "本步确定性验证失败 (Step Verification Failed)")
        }
        StagedStepPatchFailureKind::ToolMessagesNotOk => {
            let detail = staged_step_tool_failure_patch_detail(
                ctx.messages,
                ctx.step_user_index,
                ctx.acceptance,
            );
            (detail, "本步内工具调用未全部成功")
        }
    }
}

// --- Step patch recover reduce (原 step_patch_recover_reduce) ---

/// 补丁恢复入口（与 **`StepIterationReduceAction`** 两条路径对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepPatchRecoverBranch {
    OuterExecOrVerify,
    ToolFailure,
}

/// 补丁恢复计划（纯数据；供 **`StagedStepPatchRecoverSpec`** 构造）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StepPatchRecoverPlan {
    pub failure_kind: StagedStepPatchFailureKind,
    pub patch_budget: usize,
    pub steps_loop_phase: &'static str,
}

/// reduce 输出：跳过补丁轮或进入有界 **`staged_step_try_patch_recover`**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StepPatchRecoverReduceAction {
    Skip,
    Run(StepPatchRecoverPlan),
}

crate::impl_as_str!(StepPatchRecoverReduceAction, {
    Self::Skip => "skip",
    Self::Run(_) => "run_patch",
});

pub(crate) struct StepPatchRecoverReduceInput {
    pub branch: StepPatchRecoverBranch,
    pub feedback_mode: StagedPlanFeedbackMode,
    pub step_max_retries: Option<u32>,
    pub staged_plan_patch_max_attempts: usize,
    pub step_verify_failed_reason: Option<String>,
    pub has_outer_loop_error: bool,
}

pub(crate) fn reduce_step_patch_recover(
    input: StepPatchRecoverReduceInput,
) -> StepPatchRecoverReduceAction {
    match input.branch {
        StepPatchRecoverBranch::ToolFailure => {
            let patch_budget =
                staged_patch_budget_tool_messages_not_ok(input.staged_plan_patch_max_attempts);
            StepPatchRecoverReduceAction::Run(StepPatchRecoverPlan {
                failure_kind: StagedStepPatchFailureKind::ToolMessagesNotOk,
                patch_budget,
                steps_loop_phase: "patch_replanner_tool_failure",
            })
        }
        StepPatchRecoverBranch::OuterExecOrVerify => {
            if !staged_step_patch_planner_enabled(input.feedback_mode) {
                return StepPatchRecoverReduceAction::Skip;
            }
            let patch_budget = staged_patch_budget_after_step_failure(
                input.step_max_retries,
                input.staged_plan_patch_max_attempts,
            );
            if patch_budget == 0 {
                return StepPatchRecoverReduceAction::Skip;
            }
            let failure_kind = resolve_staged_step_patch_failure_kind(
                &input.step_verify_failed_reason,
                input.has_outer_loop_error,
            )
            .unwrap_or(StagedStepPatchFailureKind::OuterLoopError);
            StepPatchRecoverReduceAction::Run(StepPatchRecoverPlan {
                failure_kind,
                patch_budget,
                steps_loop_phase: "patch_replanner_attempt",
            })
        }
    }
}

#[cfg(test)]
#[path = "step_loop_tests.rs"]
mod tests;
