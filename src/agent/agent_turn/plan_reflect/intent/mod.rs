//! 回合起点意图门控：从用户消息抽取任务与廉价启发式（L2 已退役）。
//!
//! 文件拆为 `user`（用户消息侧辅助）与 `at_turn_start`（门控主逻辑）。

pub(crate) mod at_turn_start;
pub(crate) mod user {
    pub(crate) use crabmate_agent::agent_turn::intent::user::*;
}

pub(crate) use at_turn_start as intent_at_turn_start;
pub(crate) use user as intent_user;
