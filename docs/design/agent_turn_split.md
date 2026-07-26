# Agent turn 拆分（T1：外循环纯 FSM）

## 目标

把无 IO 的外循环相位 / reduce / driver / pre-gate reason 下沉到 **`crabmate-agent`**，根包只保留再导出与带副作用的 `outer_loop` / `outer_loop_reflect`。

## 本轮变更

| 模块 | 位置 |
|------|------|
| `outer_loop_fsm` | `crates/crabmate-agent/src/agent_turn/` |
| `outer_loop_iteration_reduce` | 同上 |
| `outer_loop_reflect_reason` | 同上 |
| `outer_loop_driver` | 同上 |

根包 `src/agent/agent_turn/mod.rs` 以 `pub(crate) use crabmate_agent::agent_turn::…` 保持原路径。

## 后续（未做）

- T1b：`turn_completion_decision` / `completion_suppression` 判定核下沉
- T2：`TurnSink` 拆分 `RunLoopIo` / emit
- T3：`task_level_evidence` 规则下沉
- T4：根包目录收成 `loop/` / `plan_reflect/` / `host/`
