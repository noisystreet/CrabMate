# 回合可变状态归属（设计）

**状态（2026-08）**：与运行时对齐的维护者真源；配合 **`phase_vocabulary`**（相位词汇）与 **`per_state_machine_consolidation.md`**（三机）。  
**非目标**：全局单表 FSM；复活 `hierarchy` / `staged` 编排入口。

---

## 1. 四栏归属

| 袋 | 位置 | 可写什么 | 勿塞入 |
|---|---|---|---|
| **`RunLoopTurnState`** | `src/agent/agent_turn/host/params.rs` | `messages_buf` / `messages_revision`、`sub_phase`（观测）、`TurnPlannerHints`、模型/温度覆盖、共享 **`Arc<TurnBudgetCounter>`** | Gate 计数、工作流反思 FSM、工具失败短路表 |
| **`PerCoordinator`** | `crates/crabmate-agent/src/per_coord/` | 配置镜像、`PlanRequirementSource`、**`plan_rewrite_attempts`**、workflow validate 缓存、工具失败 / `run_command` 去重、内嵌 **`WorkflowReflectionController`** | 整场只读 LLM 句柄、前端 UI 相位 |
| **`OuterLoopReflectMemo`**（暂住 `PerTurnCounters`） | `per_coord/per_turn_state.rs` | 外循环 Gate **前**纠偏：build-idle streak / 注入次数、终答缺失注入次数（R 轨 3） | 终答 Gate 相位、`plan_rewrite` |
| **`PerTurnFlight`** | `src/per_turn_flight.rs` | 只读镜像（如 `plan_rewrite_attempts`、`require_plan`）供 `/status` | 权威可变源（权威在 PerCoord） |

**消息写入约定**：会话正文以 **`RunLoopTurnState`** 为权威缓冲；`append_tool_result_and_reflection` 仍接 `&mut Vec<Message>`（历史接合），调用方须随后经 `prepare_turn_messages_for_model`（或等价路径）递增 **`messages_revision`** 并失效 workflow 层缓存。

---

## 2. R 三轨与计数

| 轨 | 可变状态 | 衔接 |
|---|---|---|
| 终答 Gate | `plan_rewrite_attempts`、`plan_requirement_source` | `after_final_assistant` / `final_plan_gate` |
| 工作流反思 | `WorkflowReflectionController` 内部相位 | `prepare_workflow_execute` / `append_*` → 置位 `PlanRequirementSource::WorkflowReflection` |
| 外循环 pre-gate | **`OuterLoopReflectMemo`** | `outer_loop_reflect`；**不**改 Gate `require_plan` |

衔接金样：`fixtures/workflow_to_plan_requirement_golden.jsonl`（`cargo test golden_workflow_to_plan_requirement`）。

---

## 3. 扩展点（挂现有 P/R，不开并行阶段机）

| 能力 | 挂点 | 预算备注 |
|---|---|---|
| **观众角色**（未实现） | 锚点 **C**（终答静态通过 / 语义检查旁）、**D**（workflow 反思决策后）；见 `audience_critic_role.md` | 侧向调用经 `complete_chat_retrying`；计入共享墙钟；独立计数名预留 `audience_calls` |
| **planner / executor 分预算**（未实现） | `OuterLoopPlanCallModelRole` 已按轮选模型端点 | 共享 **`TurnBudgetCounter`**；观测标签预留 **`LlmCallBudgetClass`**（`turn_budget.rs`），**不**改 deny 逻辑、**不**新增 TOML 键于本阶段 |

---

## 4. 与相位词汇的关系

- 编排相位字符串：`crates/crabmate-agent/src/agent_turn/phase_vocabulary.rs`
- 外循环步进只经 **`OuterLoopDriver::record_*`**
- 前端 `TurnPhase` / `StreamControlPhase` **禁止**并进本表
