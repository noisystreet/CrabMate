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

/// 一轮 Agent 的返回：成功时为结构性提交结果（供归档落盘），失败为执行错误。
pub type TurnOutcome = Result<StructuralCommit, RunAgentTurnError>;

/// 跑完整一轮 Agent 的窄注入面（参数袋仍为 [`TurnRequest`]）。
#[async_trait]
pub trait TurnEngine: Send + Sync {
    async fn run(&self, params: TurnRequest<'_>) -> TurnOutcome;
}

/// 默认实现：调用 [`crate::run_agent_turn`]。
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultTurnEngine;

#[async_trait]
impl TurnEngine for DefaultTurnEngine {
    async fn run(&self, params: TurnRequest<'_>) -> TurnOutcome {
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
