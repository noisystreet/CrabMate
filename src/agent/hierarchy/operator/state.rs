//! ReAct 循环内部状态（不对外暴露）。

use crate::types::Message;

use super::types::CompileErrorType;

/// ReAct 循环状态
#[derive(Debug, Clone)]
pub(crate) struct ReactState {
    /// 当前迭代次数
    pub iteration: usize,
    /// 历史消息
    pub messages: Vec<Message>,
    /// 观察结果
    pub observations: Vec<String>,
    /// 任务是否已完成（用于提前终止）
    pub task_completed: bool,
    /// 完成原因
    pub completion_reason: Option<String>,
    /// 当前工作目录（用于跟踪目录变化）
    pub current_working_dir: Option<std::path::PathBuf>,
    /// 连续失败计数
    pub consecutive_failures: usize,
    /// 上次失败的工具名称（用于检测重复失败）
    pub last_failed_tool: Option<String>,
    /// 上次失败的错误类型
    pub last_error_type: Option<CompileErrorType>,
    /// 最近执行的命令历史（用于检测重复命令）
    pub recent_commands: Vec<String>,
    /// 重复命令计数
    pub duplicate_command_count: usize,
    /// 轻量命令去重缓存（同一子目标内复用 `run_command cat/ls` 结果，避免重复执行）
    pub lightweight_command_cache:
        std::collections::HashMap<String, super::super::tool_executor::ToolExecutionResult>,
    /// 已使用的工具集合（用于复杂度评估）
    pub tools_used: std::collections::HashSet<String>,
    /// 按时间顺序的每次成功/失败工具调用名（与 tools_used 不同：含重复、用于验收）
    pub tool_names_chron: Vec<String>,
    /// 动态分解已触发次数
    pub dynamic_decomposition_count: usize,
    /// 循环型子目标当前阶段（轻量状态机）
    pub phase: SubgoalPhase,
    /// 收敛进展指标（仅在循环型子目标中更新）
    pub progress: ConvergenceProgress,
    /// 上次已上报到时间线的阶段（避免重复刷屏）
    pub last_reported_phase: Option<SubgoalPhase>,
}

/// 工具执行结果分析
#[derive(Debug, Clone)]
pub(crate) enum ToolExecutionOutcome {
    /// 普通执行
    Normal,
    /// 任务已完成
    TaskCompleted { reason: String },
}

/// 循环型子目标阶段（轻量状态机）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubgoalPhase {
    Diagnose,
    ApplyFix,
    Verify,
    Escalate,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ConvergenceProgress {
    pub last_error_count: Option<usize>,
    pub last_first_error_signature: Option<String>,
    pub rounds_without_progress: usize,
}
