# Agent turn 拆分

## 目标

把无 IO 的外循环相位 / reduce / driver / pre-gate reason 下沉到 **`crabmate-agent`**，根包只保留再导出与带副作用的 `outer_loop` / `outer_loop_reflect`；并把回合 IO 适配面切成可传递的控制通道与终端呈现；完成判定核亦下沉；根包目录按职责分组。

## T1（已做）：外循环纯 FSM

| 模块 | 位置 |
|------|------|
| `outer_loop_fsm` | `crates/crabmate-agent/src/agent_turn/` |
| `outer_loop_iteration_reduce` | 同上 |
| `outer_loop_reflect_reason` | 同上 |
| `outer_loop_driver` | 同上 |

根包以 `pub(crate) use crabmate_agent::agent_turn::…` 或 `loop/` 内再导出保持原语义。

## T1b（已做）：完成判定核

| 模块 | 位置 |
|------|------|
| `turn_completion_decision` | `crates/crabmate-agent/src/agent_turn/` |
| `completion_suppression` | 同上 |
| `run_command_dedupe` | 同上（根包 `host` 再导出，供 serial emit） |
| `task_level_evidence` | 同上（原计划 T3，随判定核一并下沉） |

根包 `loop/turn_completion.rs`：早停/冗余包装、终答空答纠偏文案与金样。

## T2（已做）：TurnSink 形状

| 类型 | 位置 | 职责 |
|------|------|------|
| `TurnControlSink` / `TurnTerminalIo` | `host/turn_sink.rs` | 控制面 + 终端呈现 |
| `RunLoopIo` | `host/params.rs` | `no_stream` / `cancel` + 嵌套二者 |
| `WebExecuteCtx` / emit·serial·parallel | `host/execute/` | 持有 `control: TurnControlSink` |

## T4（已做）：根包目录分组

因 `loop` 为 Rust 关键字，模块名为 **`turn_loop`**（`#[path = "loop/mod.rs"]`）。

| 目录 | 模块 | 内容 |
|------|------|------|
| `loop/` | `turn_loop` | 外循环 IO、分发、完成纠偏、`check_abort` |
| `plan_reflect/` | `plan_reflect` | `plan` / `reflect` / `intent` |
| `host/` | `host` | `execute`、`params`、`turn_sink`、`errors`、`sub_agent_policy` |

根 `mod.rs` 再导出 `errors` / `params` / `execute_tools` / `plan` / `reflect` / `intent` / `turn_completion` 等，保持 `crate::agent::agent_turn::*` 常用路径。

## 后续（执行面）

外循环纯逻辑已下沉；**IO 执行面**（`host` → `dispatch_tool`、`chat_job_queue` → `run_agent_turn`）解耦见 **`docs/design/turn_host_decouple.md`**。
