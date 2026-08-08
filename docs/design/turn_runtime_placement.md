# ADR：回合执行面落点（`crabmate-turn-runtime`）

> **状态**：**已采纳（2026-08-08）** — **暂不**新建 `crabmate-turn-runtime` / 薄接口 crate；默认实现继续留在**根包 composition root**。  
> **关联**：[`turn_host_decouple.md`](./turn_host_decouple.md) **P4**；与 Server/Client 解耦阶段 B1 对应（中间稿已收工，结论以本文为准）。  
> **非目标**：本 ADR **不**改 SSE/HTTP 字段，**不**搬迁 `chat_job_queue`（属 P5）。

---

## 1. 背景

P1–P3 已落地：

| 能力 | 位置 |
|------|------|
| `TurnRunner` / `DefaultTurnRunner` | 根包 `src/turn_runner.rs` |
| `ToolDispatch` / `InternalToolDispatch` | 根包 `src/agent/agent_turn/host/execute/tool_dispatch.rs` |
| 默认装配 | `DefaultTurnRunner` → `run_agent_turn`；queue 经 `Arc<dyn TurnRunner>` 注入 |
| 禁边 | `scripts/check-crate-deps.sh`：`agent` / `workflow` / `tools` / `web-host` **↛** `internal` |

P4 原问题：是否把「默认执行面」迁到独立 crate（如 `crabmate-turn-runtime`），使根包只做薄装配。

---

## 2. 决策

**采纳选项 A：维持现状（根包 composition root）。**

- **不**新增 `crates/crabmate-turn-runtime`。
- **不**新增仅含 trait 的 `crabmate-turn-api`（见 §3 否决理由）。
- 入口继续只依赖 `TurnRunner`；默认实现继续在根包转发 `run_agent_turn`。
- P5（queue/handler 贴近 web-host）**不**以「先建 turn-runtime」为前置；若迁出受阻，再按 §5 触发条件重开本决策。

---

## 3. 选项对比

| 选项 | 做法 | 收益 | 代价 / 风险 |
|------|------|------|-------------|
| **A. 根包 composition root（采纳）** | 保持 `turn_runner.rs` + `run_agent_turn` 在根包 | 零迁移成本；与当前 queue/CLI/TUI/Web 同进程装配一致；禁边已够用 | 根包仍大；独立测默认实现仍需链根包 |
| B. `crabmate-turn-runtime` | 默认 `TurnRunner` + 对 `run_agent_turn`/工具宿主的装配迁入新 crate | 根包变薄；名义上「runtime」可复用 | `RunAgentTurnParams` / 错误类型 / agent FSM / internal 工具边几乎整棵拖入 → **新 crate ≈ 今日根包**；禁边与 workspace 图变复杂；无第二消费者时 ROI 低 |
| C. 薄接口 crate（仅 trait） | 如 `crabmate-turn-api` 只放 `TurnRunner` | 理论上下游只依赖接口 | `run` 签名依赖 `RunAgentTurnParams` / `RunAgentTurnError`（根包或 agent）；要么参数袋再拆一轮（远超 P4），要么「薄」crate 立刻变厚；**queue 仍在根包**，拆 trait  alone 不解 P5 |

---

## 4. 理由（为何现在选 A）

1. **注入边已切开**：`chat_job_queue` 不再直调 `run_agent_turn`；测试可 mock `TurnRunner` / `ToolDispatch`。P4 的「可替换」在**进程内**已满足。
2. **缺第二装配消费者**：尚无「只要回合、不要 serve 根包」的独立二进制强需求；飞书桥 / IDE 等若接入，优先挂现有 HTTP 或同进程 `TurnRunner`（见 server/client 解耦非目标）。
3. **搬家成本集中在参数袋与 internal**：真正阻碍「小 runtime crate」的是巨大的 `RunAgentTurnParams` 与工具 registry，不是缺一个空 crate 壳。
4. **与契约优先一致**：Server/Client 解耦近期优先 HTTP/SSE 契约卫生；执行面 crate 搬家不降低远程 Client 风险。

---

## 5. 重开条件（何时再议 B/C）

满足**任一**即可重新评估（新 ADR 或修订本文状态）：

1. **`chat_job_queue`（或等价 worker）迁出根包**（P5），且目标 crate **不能**依赖根包类型，必须依赖可独立版本化的 `TurnRunner` 面。
2. 出现**第二二进制**必须链接回合执行、又明确拒绝依赖 `crabmate` 根包（且 HTTP Client 路径不可接受）。
3. 完成 **`RunAgentTurnParams` 密封 / 稳定子集**（builder-only 或独立 DTO crate）后，薄接口 crate 的依赖闭包变得可接受。

在此之前：**禁止**为「目录更干净」而先行创建空的 `crabmate-turn-runtime`。

---

## 6. 对后续工作的含义

| 工作 | 含义 |
|------|------|
| **P5** | 可直接评估 queue/handler 贴近 `web-host`；依赖本决策：默认 runner **仍由根包提供** `Arc<dyn TurnRunner>` 注入，不必等 runtime crate |
| **新入口**（bench / 桥） | 优先 `default_turn_runner()` 或自备 `TurnRunner` mock；勿复制 `agent_turn` |
| **文档** | `turn_host_decouple.md` P4 标为「已决策 → 见本文」；开发文档设计索引链到本文 |

---

## 7. 成功一句话（与既有 design 对齐）

**入口只认识 TurnRunner；默认实现继续住在根包 composition root——直到出现真实的第二装配边界，再拆 `turn-runtime`，而不是先为拆而拆。**
