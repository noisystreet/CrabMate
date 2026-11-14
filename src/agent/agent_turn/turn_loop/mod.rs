//! 外循环与回合分发（IO 侧）：outer_loop、ReAct driver、完成纠偏包装。
//!
//! 纯 FSM / reduce / decision 在 **`crate::agent::agent_turn::policy`**；本目录只承载副作用。
//!
//! **不再**为 policy 子模块开同名转发（`turn_loop::outer_loop_fsm` 之类）：同一个类型有两条
//! 合法路径时，「该用哪条」没有答案，且新增 policy 子模块时漏加转发不会报错。需要时直接写
//! `crate::agent::agent_turn::policy::…`。

pub(crate) mod check_abort;
pub(crate) mod context_timeline_sse;
pub(crate) mod orchestration_route;
pub(crate) mod outer_loop;
pub(crate) mod outer_loop_build_idle;
pub(crate) mod outer_loop_reflect;
pub(crate) mod react_turn;
pub(crate) mod run_dispatch;
pub(crate) mod turn_completion;
