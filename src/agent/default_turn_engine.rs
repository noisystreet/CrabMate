//! `TurnEngine` 的默认实现：转发到 [`crate::run_agent_turn`]。
//!
//! 归属**实现侧**（`agent`）而非端口层 `turn_runner`（#1510）：默认引擎必然包含
//! 「调哪个回合函数」这个实现决策，留在端口层会让 `turn_runner → run_agent_turn`
//! 成为一条装配泄漏的边（端口层依赖具体实现）。端口层只留 `TurnEngine` /
//! `TurnStats` / `TurnOutcome`。
//!
//! 根上的承诺面路径不变：`crate::DefaultTurnEngine` / `crate::default_turn_engine()`
//! 仍由 `lib.rs` 从这个模块 re-export，见 `fixtures/public_api_golden.txt`。

use async_trait::async_trait;

use crate::TurnRequest;
use crate::turn_error::RunAgentTurnError;
use crate::turn_runner::{TurnEngine, TurnOutcome};

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
