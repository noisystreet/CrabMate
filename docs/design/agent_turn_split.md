# Agent turn 拆分

> **目录现状（P1 布局收敛后）**：全部落在根包 **`src/agent/agent_turn/`** 的四个子目录下；
> 纯逻辑（原 `cm_agent/agent_turn/`）已整体迁入 **`policy/`**。见
> [`src_layout_shim_cleanup.md`](./src_layout_shim_cleanup.md) 的 P1 与
> [`crates_io_single_package.md`](./crates_io_single_package.md)（单包化后「`cm_agent` 与根包」不再是两个 crate，
> 同一逻辑模块不宜留两棵同名目录）。

## 目标

把无 IO 的外循环相位 / reduce / driver / pre-gate reason 与完成判定核，与带副作用的执行面**按副作用边界**分开，
但**同处根包**：纯逻辑进 `agent_turn/policy/`，IO 侧留 `agent_turn/{host,turn_loop,plan_reflect}`；
并把回合 IO 适配面切成可传递的控制通道与终端呈现。

> **历史**：T1 / T1b 最初把纯逻辑放在 `cm_agent/agent_turn/`（当时 `cm_agent` 是独立 crate，`cm_agent` 无根包依赖可强制）。
> 单包化后这道 crate 边界消失，该分离只剩「物理位置」意义，且与根包 `agent/agent_turn/` 形成两棵同名目录；
> P1 已将前者整体迁入后者 `policy/`，`agent_turn` 现在只有一处物理位置。

## T1（已做）：外循环纯 FSM

| 模块 | 位置 |
|------|------|
| `outer_loop_fsm` | `src/agent/agent_turn/policy/` |
| `outer_loop_iteration_reduce` | 同上 |
| `outer_loop_reflect_reason` | 同上 |
| `outer_loop_driver` | 同上 |

`turn_loop/mod.rs` 与 `host/mod.rs` 以 `pub(crate) use crate::agent::agent_turn::policy::…` 再导出，保持原语义。

## T1b（已做）：完成判定核

| 模块 | 位置 |
|------|------|
| `turn_completion_decision` | `src/agent/agent_turn/policy/` |
| `completion_suppression` | 同上 |
| `run_command_dedupe` | 同上（`host` 再导出，供 serial emit） |
| `task_level_evidence` | 同上（原计划 T3，随判定核一并迁移） |

`turn_loop/turn_completion.rs`：早停/冗余包装、终答空答纠偏文案与金样。

## T2（已做）：TurnSink 形状

| 类型 | 位置 | 职责 |
|------|------|------|
| `TurnControlSink` / `TurnTerminalIo` | `host/turn_sink.rs` | 控制面 + 终端呈现 |
| `RunLoopIo` | `host/params.rs` | `no_stream` / `cancel` + 嵌套二者 |
| `WebExecuteCtx` / emit·serial·parallel | `host/execute/` | 持有 `control: TurnControlSink` |

## T4（已做）：目录分组

四个子目录与模块一一同名，**不再需要 `#[path]`**（原 `loop/` 因 `loop` 是 Rust 关键字而以
`#[path = "loop/mod.rs"] mod turn_loop;` 挂载；P1 已把目录改名为 `turn_loop/`）。

| 目录 | 模块 | 内容 |
|------|------|------|
| `turn_loop/` | `turn_loop` | 外循环 IO、分发、完成纠偏、`check_abort` |
| `plan_reflect/` | `plan_reflect` | `plan` / `reflect` / `intent` |
| `host/` | `host` | `execute`、`execute_tools`、`params`、`run_command_dedupe`、`turn_sink`、`errors` |
| `policy/` | `policy` | 外循环 FSM / reduce / driver / pre-gate reason、完成判定核、编排相位词表、`task_level_evidence`；**不触 IO**，只依赖 `cm_agent` 的纯类型 |

根 `mod.rs` 再导出 `errors` / `params` / `execute_tools` / `plan` / `reflect` / `intent` / `turn_completion` 等，
保持 `crate::agent::agent_turn::*` 常用路径。`policy` 以 `#[doc(hidden)] pub mod` 声明，与迁移前
`#[doc(hidden)] pub mod cm_agent::agent_turn` 的可见性等价（公开可达、不进 rustdoc）。

## 后续（执行面）

外循环纯逻辑已与执行面同处 `agent_turn/` 并保持副作用边界；**IO 执行面**（`host` → `dispatch_tool`、
`chat_job_queue` → `run_agent_turn`）解耦见 **`docs/design/turn_host_decouple.md`**。
