//! 可注入的回合执行面：打断 Web 队列对 [`crate::run_agent_turn`] 的硬连。
//!
//! 入口（`chat_job_queue` worker）只依赖 [`TurnRunner`]；默认实现仍转发到根包
//! [`crate::run_agent_turn`]。见 **`docs/design/turn_host_decouple.md`** P2；
//! 执行面落点（暂不拆 `crabmate-turn-runtime`）见 **`docs/design/turn_runtime_placement.md`**。

use async_trait::async_trait;

use crate::RunAgentTurnParams;
use crate::agent::agent_turn::RunAgentTurnError;

/// 跑完整一轮 Agent 的窄注入面（参数袋仍为 [`RunAgentTurnParams`]）。
#[async_trait]
pub trait TurnRunner: Send + Sync {
    async fn run(&self, params: RunAgentTurnParams<'_>) -> Result<(), RunAgentTurnError>;
}

/// 默认实现：调用 [`crate::run_agent_turn`]。
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultTurnRunner;

#[async_trait]
impl TurnRunner for DefaultTurnRunner {
    async fn run(&self, params: RunAgentTurnParams<'_>) -> Result<(), RunAgentTurnError> {
        crate::run_agent_turn(params).await
    }
}

/// serve / 测试装配用的默认 [`TurnRunner`] 句柄。
#[must_use]
pub fn default_turn_runner() -> std::sync::Arc<dyn TurnRunner> {
    std::sync::Arc::new(DefaultTurnRunner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_turn_runner_is_object_safe_send_sync() {
        let r = default_turn_runner();
        fn assert_bounds(_: &(dyn TurnRunner + Send + Sync)) {}
        assert_bounds(r.as_ref());
    }
}
