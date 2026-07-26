//! 执行宿主与回合 IO 形状：工具批、SSE 控制面、`RunLoopParams`、错误类型。

pub(crate) mod errors;
pub(crate) mod execute;
pub(crate) mod params;
pub(crate) mod sub_agent_policy;
pub(crate) mod turn_sink;

pub(crate) mod run_command_dedupe {
    pub(crate) use crabmate_agent::agent_turn::run_command_dedupe::*;
}

pub(crate) use execute::tools as execute_tools;
