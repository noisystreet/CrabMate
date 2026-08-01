# 回合宿主解耦（Turn Host Decouple）

## 状态

- **规划入库**：本文（PR-T0）
- **相关已合**：agent_turn FSM / Sink（#696）、web-host A/B/C（#697）
- **P1 / P2 / P3a**：已落地 `ToolDispatch`、`TurnRunner`，以及 `DispatchToolParams` 嵌套分组
- **P3b**：窄控制面 `WebChatAppFacet`（approval / branch / messages / conversation-store）
- **P3c**：回合面 `WebChatTurnAppFacet`；`POST /chat`、`/chat/stream`、`/chat/async`、job status 与 cron 入队已迁入
- **未做**：P3d（`RunAgentTurnParams` 入口子集）、P4 执行面落点、P5 handler/queue 迁出评估

## 目标

把「能跑完整回合」变成可注入边界，打断两处焊死关系：

1. `chat_job_queue` → 同 crate 硬连 `run_agent_turn`
2. `agent_turn/host` → `crabmate-internal::tool_registry::dispatch_tool`

若不切开，handler 迁 web-host、agent 独立跑完整回合、继续收窄 `AppState` 都会卡住。

## 非目标

- 拆独立 Web / Agent 微服务或独立仓库
- 改 SSE 行协议或对外 HTTP JSON 字段
- 把 `run_agent_turn` **整文件**一次性搬进 `crabmate-agent`（立刻撞 `internal` 禁边）
- 本阶段拆分 `crabmate-tools` 为多 crate
- 为解耦引入动态插件 / FFI

## 目标依赖方向

```text
入口（Web queue / CLI / TUI / bench）
  │  只依赖 TurnRunner
  ▼
TurnRunner 实现（默认仍在根包 composition root）
  │  编排：crabmate-agent FSM + 完成判定
  │  IO：TurnControlSink / TurnTerminalIo / TurnProcessHandles
  ▼
ToolDispatch（窄面；挂在已有 ToolExecutionHost 之下）
  │  默认实现 → internal::dispatch_tool
  ▼
crabmate-internal / crabmate-tools
```

约束（`scripts/check-crate-deps.sh`，已挂 pre-commit / CI）：

- `crabmate-agent` / `workflow` / `tools` / `approval` / `web-host` **↛** `crabmate-internal`
- **不要**把 `ToolDispatch` / `DispatchToolParams` 放进 `crabmate-agent`（禁边 + 参数袋会拖垮领域包）

## 阶段

| 阶段 | 内容 | 预估 |
|------|------|------|
| **P0** | 本文入库；禁边脚本进门禁 | **完成**（本文） |
| **P1** | 根包 `ToolDispatch` + 默认 adapter；`ToolExecutionHost` 间接调 registry；补 mock Dispatch 测 | **完成**（本分支） |
| **P2** | `TurnRunner`；`WebChatQueueDeps` 注入；queue **禁止**直接 `run_agent_turn` | **完成**（本分支） |
| **P3** | 参数袋按片收窄：`DispatchToolParams` 嵌套（**P3a**）；窄 chat facet（**P3b**）；回合 Turn facet（**P3c**）；`RunAgentTurnParams` 入口子集（P3d） | 多 PR |
| **P4** | 评估执行面落点：默认根包 composition root；条件成熟再 `crabmate-turn-runtime` / 薄接口 crate | 设计决策 |
| **P5** | 红利：更多 handler/queue 贴近 web-host；`tools` 拆子包等（另开 PR） | 后续 |

### P1 注意

已有根包 **`ToolExecutionHost`**（非 `crabmate-agent` 内同名领域批执行）。P1 在其**下**挂窄 `ToolDispatch`，避免平行再造第二套「执行宿主」命名。

### P2 注意

只解**调用边**；`RunAgentTurnParams` 仍可很大。完成后更新 **`web_host_extract.md`**：非目标「不可迁 queue」改为「可评估」。

### P3 注意

`RunLoopParams` 已拆 `Core` / `Io` / `Attach` / `Obs`（见 `host/params.rs`）。  
**P3a（已完成）**：`DispatchToolParams` 顶层改为 `call` / `workspace` / `policy` / `obs` / `memory` 嵌套（字段不删）。  
**P3b（已完成）**：`WebChatAppFacet`（`cfg` + `conversation` + `approval_sessions`）+ 窄控制面路由。  
**P3c（已完成）**：`WebChatTurnAppFacet`（`cfg` / `api_key: Arc<str>` / `client` / workspace / `conversation` / `chat` queue / approval / `process_handles` / SSE hub / `async_chat_jobs`；**不含** `tools`/uploads）；`POST /chat`、`/chat/stream`、`/chat/async`；job status 用更窄的 `AsyncChatJobsFacet`；enqueue/turn_build/cron 经 Turn 面。  
**P3d**：`RunAgentTurnParams` 入口强制子集 / 嵌套（勿静默改 HTTP/SSE 契约）。

## 建议 PR 切片

| PR | 内容 | 依赖 |
|----|------|------|
| T0 | 本文 + 禁边门禁 | — |
| T1 | `ToolDispatch` adapter + mock 测 | T0 |
| T2a | `TurnRunner` + queue 注入 | T1 |
| T2b | 更新 `web_host_extract` / `crate_dep_policy` 文案 | T2a |
| T3… | 参数袋 / chat facet | T2a |
| T4 | （可选）接口 crate / turn-runtime | T3 后 |

## 验收清单（规划级）

- [x] `chat_job_queue` 不直接调用 `run_agent_turn`（经 `TurnRunner`）
- [x] `agent_turn/host` 不直接 `use …::dispatch_tool`（经 `ToolDispatch` / `InternalToolDispatch`）
- [x] 存在 mock `ToolDispatch` 最小分发测试（外循环 mock 留待后续）
- [x] 禁边脚本在 pre-commit 与 CI 中执行
- [ ] 对外 SSE / HTTP 字段无静默变更
- [ ] CLI / TUI / Web 各至少一次真实回合冒烟

## 关键路径

| 路径 | 角色 |
|------|------|
| `src/run_agent_turn.rs` | 对外装配入口 |
| `src/turn_runner.rs` | `TurnRunner` / `DefaultTurnRunner` |
| `src/agent/agent_turn/host/params.rs` | `RunLoopParams` 等 |
| `src/agent/agent_turn/host/execute/tool_execution_host.rs` | 经 `ToolDispatch` 调 registry |
| `src/agent/agent_turn/host/execute/tool_dispatch.rs` | `ToolDispatch` / `InternalToolDispatch` |
| `src/agent/agent_turn/host/execute/tool_execution_trait.rs` | `ToolExecutionHost` |
| `crates/crabmate-internal/src/tool_registry/` | `dispatch_tool` / `DispatchToolParams` |
| `src/chat_job_queue/worker/` | Web 异步回合消费者 |
| `src/web/app_state.rs` / `app_state_facets.rs` | Web 状态与 facet（含 **`WebChatAppFacet`**） |
| `scripts/check-crate-deps.sh` | 禁边门禁 |

## 与既有文档

| 文档 | 关系 |
|------|------|
| [`agent_turn_split.md`](./agent_turn_split.md) | 已完成 FSM / Sink；本文接执行面 |
| [`crate_dep_policy.md`](./crate_dep_policy.md) | 禁边与 facet；本文实现其「后续」中的 runner 注入 |
| [`web_host_extract.md`](./web_host_extract.md) | A/B/C 已完成；本文解除「handler 不能迁」的根因 |

## 成功一句话

入口只认识 **TurnRunner**；回合只认识 **ToolDispatch**；`internal` 只做默认实现——根包从唯一上帝路径变成可替换的 composition root。
