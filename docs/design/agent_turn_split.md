# Agent turn 拆分

## 目标

把无 IO 的外循环相位 / reduce / driver / pre-gate reason 下沉到 **`crabmate-agent`**，根包只保留再导出与带副作用的 `outer_loop` / `outer_loop_reflect`；并把回合 IO 适配面切成可传递的控制通道与终端呈现。

## T1（已做）：外循环纯 FSM

| 模块 | 位置 |
|------|------|
| `outer_loop_fsm` | `crates/crabmate-agent/src/agent_turn/` |
| `outer_loop_iteration_reduce` | 同上 |
| `outer_loop_reflect_reason` | 同上 |
| `outer_loop_driver` | 同上 |

根包 `src/agent/agent_turn/mod.rs` 以 `pub(crate) use crabmate_agent::agent_turn::…` 保持原路径。

## T2（已做）：TurnSink 形状

| 类型 | 位置 | 职责 |
|------|------|------|
| `TurnControlSink` | `src/agent/agent_turn/turn_sink.rs` | SSE `out`、编码器、镜像、工具批 / 澄清钩子 |
| `TurnTerminalIo` | 同上 | `render_to_terminal`、plain stream、TUI scratch |
| `RunLoopIo` | `params.rs` | `no_stream` / `cancel` + 嵌套上述二者 |
| `WebExecuteCtx` / `ExecuteToolsCommonCtx` | `execute/tools` | 持有 `control: TurnControlSink` |
| `EmitToolResultParams` / serial·parallel 状态 | 同上 | 控制面字段收成 `control`（底层 `emit_*` 自由函数仍吃扁平 `out`/`encoder`） |

入口装配：`run_agent_turn.rs`；宏 `check_abort!` 读 `io.control.out`。

## 后续（未做）

- T1b：`turn_completion_decision` / `completion_suppression` 判定核下沉
- T3：`task_level_evidence` 规则下沉
- T4：根包目录收成 `loop/` / `plan_reflect/` / `host/`
