# ADR：P5 — `chat_job_queue` / chat handler 贴近 web-host

> **状态**：**已评估（2026-08-08）** — **暂不**整包迁 `chat_job_queue` 或带状态 chat handler 入 `crabmate-web-host`。  
> **关联**：[`web_host_extract.md`](./web_host_extract.md)、[`turn_host_decouple.md`](./turn_host_decouple.md) P5、[`turn_runtime_placement.md`](./turn_runtime_placement.md)（P4）、[`crate_dep_policy.md`](./crate_dep_policy.md)。  
> **非目标**：改 SSE/HTTP 字段；为搬家而新建空 `crabmate-turn-runtime`（P4 已否决）。

---

## 1. 背景

P2 已切开调用边：`chat_job_queue` worker 只经 `Arc<dyn TurnRunner>` 执行回合，**禁止**直调 `run_agent_turn`。  
web-host A/B/C 已完成：DTO / `GET /web-ui` / 体积分层与静态挂载。

P5 原问题：能否把 queue 或更多 chat handler **物理迁入** `crabmate-web-host`，进一步收窄根包。

---

## 2. 决策

**采纳：维持模块边界；「贴近」= 依赖方向清晰，不是整包搬家。**

| 模块 | 决策 |
|------|------|
| `src/chat_job_queue/` | **留根包** |
| `src/web/chat_handlers/`（含带 `State<Facet>` 的路由） | **留根包** |
| `crabmate-web-host` | 继续专责 **HTTP 契约 + serve 壳 + 无状态路由** |

短期目标态：

```text
根包 composition root
  AppState / Facets / 域 handler / ChatJobQueue / DefaultTurnRunner
       │
       ▼
crabmate-web-host
  DTO · limits · keys · GET /web-ui · body limit · SPA/uploads 静态挂载
```

---

## 3. 为何现在不能整包迁

### 3.1 循环依赖（S0）

根包已 optional 依赖 `crabmate-web-host`。  
queue / 回合 handler 需要根包上的 `TurnRunner`、`RunAgentTurnParams`、会话 facet 等。  
若迁入 web-host 再依赖根包 → **Cargo 循环**。  
P4 已说明：仅抽薄 trait crate **不解**此题（参数袋仍绑根包/agent）。

### 3.2 `web-host ↛ internal` 禁边（S0/S1）

queue setup 经 `tool_registry` 门面构造 `WebToolRuntime`，并拉角色/会话模式/记忆/变更集等（多经根包再导出 internal）。  
迁入 web-host 要么破 `check-crate-deps.sh`，要么先把这些能力下沉到**允许**依赖的 crate——工作量远超「搬目录」。

### 3.3 axum `FromRef` 孤儿规则（S0）

`FromRef<Arc<AppState>> for WebChat*Facet` 要求 Facet 与 `AppState` **同 crate**。  
`AppState` 在根包 → 带状态 handler 无法合法放进 web-host（见 `web_host_extract.md`「为何 handler 未整包迁入」）。

### 3.4 与 P4 一致

默认 `DefaultTurnRunner` **必须**留在能链到 `run_agent_turn` → agent → `ToolDispatch` → internal 的 composition root。  
web-host 只应拿注入句柄，不应承载默认实现。

---

## 4. 现在可做的「贴近」（低风险）

1. **契约与无状态壳继续进 web-host**（已完成路径）：新 DTO / 无 `AppState` 的路由按 `GET /web-ui` 模式扩展。  
2. **根包内继续收窄 Facet**：回合面已不含 tools/uploads；新字段优先挂窄 facet，避免回膨胀 `AppState`。  
3. **保持 queue 只认 `TurnRunner`**：回归勿恢复直调 `run_agent_turn`。  
4. **可选小清理**：构造 `WebToolRuntime` 时减少对 internal `tool_registry` 门面的字面依赖（直接 `crabmate_tools`）——减噪，**单独不能**解锁迁 crate。  
5. **文档**：以本文为 P5 评估结论；勿在待办里重复「必须先建 turn-runtime」。

**本阶段不做**：为 P5 新建 crate、改 HTTP/SSE、强行跨 crate 实现 `FromRef`。

---

## 5. 重开条件（何时再议整包迁）

满足**多项**后再开迁移 PR（可修订本文状态）：

1. **可独立依赖的 Turn 入口**：密封/`builder` 子集或 DTO crate，使 worker 不必依赖整包 `crabmate`（与 [`turn_runtime_placement.md`](./turn_runtime_placement.md) §5 同向）。  
2. **`AppState` / Facet 归属策略**：迁入 web-host，或放弃孤儿 `FromRef`、改用显式提取 / 新状态宿主 crate。  
3. **internal 能力旁路**：角色/会话模式/记忆/健康元数据等不再迫使 web-host 触 internal。  

在此之前：**禁止**为「目录更干净」把 queue 或回合 handler 硬迁进 web-host。

---

## 6. 对后续工作的含义

| 工作 | 含义 |
|------|------|
| Server/Client 解耦 | 远程 Client 不依赖本 P5；继续守 HTTP/SSE 契约即可 |
| 新 chat 路由 | 默认仍落 `src/web/`；仅无状态壳可进 web-host |
| P4 | 不冲突：执行面留根包；P5 搬家另需 §5 前置 |
| 冒烟 / runbook | B3 清单见 [`client_turn_smoke_runbook.md`](./client_turn_smoke_runbook.md)；与模块搬家正交 |

---

## 7. 成功一句话

**调用边已解（TurnRunner）；模块边仍被「根包 composition root + internal 禁边 + FromRef」锁死——P5 现阶段巩固边界与契约壳，整包搬家等到独立 Turn DTO 与 AppState 归属方案就绪。**
