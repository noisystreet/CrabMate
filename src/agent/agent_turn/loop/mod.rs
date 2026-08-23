//! 外循环与回合分发（IO 侧）：outer_loop、ReAct driver、完成纠偏包装。
//!
//! 纯 FSM / reduce / decision 在 **`crabmate-agent::agent_turn`**；本目录再导出并承载副作用。

pub(crate) mod check_abort;
pub(crate) mod context_timeline_sse;
pub(crate) mod orchestration_route;
pub(crate) mod outer_loop;
pub(crate) mod outer_loop_build_idle;
pub(crate) mod outer_loop_reflect;
pub(crate) mod react_turn;
pub(crate) mod run_dispatch;
pub(crate) mod turn_completion;

pub(crate) mod outer_loop_driver {
    pub(crate) use crate::cm_agent::agent_turn::outer_loop_driver::*;
}
pub(crate) mod outer_loop_fsm {
    pub(crate) use crate::cm_agent::agent_turn::outer_loop_fsm::*;
}
pub(crate) mod outer_loop_iteration_reduce {
    pub(crate) use crate::cm_agent::agent_turn::outer_loop_iteration_reduce::*;
}
pub(crate) mod outer_loop_reflect_reason {
    pub(crate) use crate::cm_agent::agent_turn::outer_loop_reflect_reason::*;
}
pub(crate) mod orchestration_entry {
    pub(crate) use crate::cm_agent::agent_turn::orchestration_entry::*;
}
