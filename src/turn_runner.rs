//! 可注入的回合执行面：打断 Web 队列对 [`crate::run_agent_turn`] 的硬连。
//!
//! 入口（`chat_job_queue` worker）只依赖 [`TurnEngine`]；默认实现仍转发到根包
//! [`crate::run_agent_turn`]。见 **`docs/design/turn_host_decouple.md`** P2；
//! 执行面落点（暂不拆 `crabmate-turn-runtime`）见 **`docs/design/turn_runtime_placement.md`**。
//!
//! **命名说明**（S2，见 #1285）：类型已正名为 [`TurnEngine`] / [`DefaultTurnEngine`] /
//! [`default_turn_engine`]，但**模块名与文件名仍保留 `turn_runner`**（历史名）——
//! 本片只做类型正名，不搬文件，避免把「改名」和「搬移」混进同一个 diff。
//! 三者均为 `pub(crate)`，不在承诺面上，故不保留旧类型名别名。

use async_trait::async_trait;

use crate::TurnRequest;
use crate::agent::agent_turn::RunAgentTurnError;
use crate::agent::model_context_view::StructuralCommit;

/// 本轮的只读聚合（全部取自既有账本，不引入任何新语义、不参与业务判定）。
///
/// - `llm_calls` / `outer_loop_iterations` / `estimated_tokens` / `elapsed_secs` / `degraded`
///   来自 [`crate::cm_agent::turn_budget::TurnBudgetCounter`]——回合内本就一直在累计。
/// - `truncated` 来自 [`crate::per_turn_flight::PerTurnFlight::last_assistant_finish_truncated`]，
///   即「层 1 截断信号」，与本仓 Web 侧 `stream_finish` 读的是同一个位。
///
/// **刻意不提供「本轮工具调用次数」**：工具账本（`ToolOutcomeRecorder`）是**进程级累计**，
/// 没有单轮维度；在这里凑一个近似值会得到一个与回合无关的数字，比没有更糟。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TurnStats {
    pub llm_calls: u32,
    pub outer_loop_iterations: u32,
    pub estimated_tokens: usize,
    pub elapsed_secs: u64,
    /// 预算降级是否已激活（`TurnBudgetCounter::is_degradation_active`）。
    pub degraded: bool,
    /// 本轮最后一次 assistant 输出是否被截断（`finish_reason=length` 的层 1 信号）。
    pub truncated: bool,
}

impl TurnStats {
    /// 从回合账本取快照。`truncated` 由调用方从 flight 读取后传入——它不归预算计数器管。
    #[must_use]
    pub fn from_budget(
        budget: &crate::cm_agent::turn_budget::TurnBudgetCounter,
        truncated: bool,
    ) -> Self {
        Self {
            llm_calls: budget.llm_calls(),
            outer_loop_iterations: budget.outer_loop_iterations(),
            estimated_tokens: budget.estimated_tokens(),
            elapsed_secs: budget.elapsed_secs(),
            degraded: budget.is_degradation_active(),
            truncated,
        }
    }
}

/// 一轮 Agent 的返回：结构性提交 + 只读聚合。
///
/// 与 `docs/design/sdk_facade.md` §5.1 的 `Result<TurnOutcome, TurnError>` 对齐。
/// **错误路径不携带聚合**：中断 / 取消时的部分输出由调用方从 `messages` 与 SSE 侧读取，
/// 与本结构无关（既有语义，未改动）。
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    /// 成功时的结构性提交结果（供归档落盘）。
    pub commit: StructuralCommit,
    /// 本轮只读聚合。
    pub stats: TurnStats,
}

/// 跑完整一轮 Agent 的窄注入面（参数袋仍为 [`TurnRequest`]）。
#[async_trait]
pub trait TurnEngine: Send + Sync {
    async fn run(&self, params: TurnRequest<'_>) -> Result<TurnOutcome, RunAgentTurnError>;
}

/// 默认实现：调用 [`crate::run_agent_turn`]。
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultTurnEngine;

#[async_trait]
impl TurnEngine for DefaultTurnEngine {
    async fn run(&self, params: TurnRequest<'_>) -> Result<TurnOutcome, RunAgentTurnError> {
        crate::run_agent_turn(params).await
    }
}

/// serve / 测试装配用的默认 [`TurnEngine`] 句柄。
#[must_use]
pub fn default_turn_engine() -> std::sync::Arc<dyn TurnEngine> {
    std::sync::Arc::new(DefaultTurnEngine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_turn_engine_is_object_safe_send_sync() {
        let r = default_turn_engine();
        fn assert_bounds(_: &(dyn TurnEngine + Send + Sync)) {}
        assert_bounds(r.as_ref());
    }
}
