//! 回合起点意图门控：从用户消息抽取任务、L2 优先管线与非分层模式的「开局」门控。
//!
//! 文件拆为 `user`（用户消息侧辅助）与 `at_turn_start`（门控主逻辑）。

pub(crate) mod at_turn_start;
pub(crate) mod l2_classifier_host;
pub(crate) mod user {
    pub(crate) use crabmate_agent::agent_turn::intent::user::*;
}

pub(crate) use at_turn_start as intent_at_turn_start;
#[allow(unused_imports)]
pub(crate) use crabmate_agent::agent_turn::intent::readonly_overview_bypass;
pub(crate) use user as intent_user;
