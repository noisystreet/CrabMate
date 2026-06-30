//! 单 Agent **`run_agent_outer_loop`** 迭代相位与反思分支（`docs/design/per_state_machine_consolidation.md` P/R/E 外层）。
//! IO 与 LLM 调用留在 [`super::outer_loop`]；本模块仅类型与 `tracing` 字符串。

/// 单 Agent 外循环内一次迭代的**粗粒度**阶段（与 `AgentTurnSubPhase` 正交，仅用于 `tracing` 排障）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OuterLoopIterationPhase {
    /// 通过迭代守卫后、准备 planner 上下文前（[`OuterLoopPlanCallModelRole`] 已应用到 `use_executor_model`）。
    IterationEnter,
    /// `prepare_messages_for_model` 等准备完成，即将 `per_plan_call_model_retrying`。
    PrepareContextDone,
    /// 已 `push` assistant，即将反思或（若 `ProceedToTools`）工具轮。
    AfterPlannerModel,
    /// 反思分支已决（不进入工具 / 重开一轮 / 去工具）。
    ReflectDecided,
    /// `per_execute_tools_web` 工具批。
    ToolsExecute,
}

impl OuterLoopIterationPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::IterationEnter => "iteration_enter",
            Self::PrepareContextDone => "prepare_context_done",
            Self::AfterPlannerModel => "after_planner_model",
            Self::ReflectDecided => "reflect_decided",
            Self::ToolsExecute => "tools_execute",
        }
    }
}

/// 单次外层迭代结束后的显式去向（替代隐式 `break` / `continue`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OuterLoopIterationExit {
    /// 进入下一轮 `per_plan_call_model_retrying`（含规划重写 `continue` 语义）。
    ContinueNextIteration,
    /// 结束 `run_agent_outer_loop`（正常停轮、取消、`BreakOuter` 等）。
    StopOuterLoop,
}

impl OuterLoopIterationExit {
    pub(crate) fn as_trace_str(self) -> &'static str {
        match self {
            Self::ContinueNextIteration => "continue_next_iteration",
            Self::StopOuterLoop => "stop_outer_loop",
        }
    }
}

/// `per_reflect_after_assistant` 结果映射为外循环控制（见 [`super::outer_loop`]）。
#[derive(Debug)]
pub(crate) enum ReflectBranchCtl {
    /// 结束外层循环（正常停轮或规划重写耗尽已处理 SSE）。
    BreakOuter,
    /// `continue 'outer`（规划重写）。
    ContinueOuter,
    /// 进入工具执行阶段。
    ProceedToTools,
}

impl ReflectBranchCtl {
    pub(crate) fn as_trace_str(&self) -> &'static str {
        match self {
            Self::BreakOuter => "break_outer",
            Self::ContinueOuter => "continue_outer",
            Self::ProceedToTools => "proceed_to_tools",
        }
    }
}
