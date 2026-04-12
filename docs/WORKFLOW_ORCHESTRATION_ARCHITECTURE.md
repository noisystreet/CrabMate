# 工作流编排扩展：状态机、条件与循环（架构设计）

**状态**：设计稿（**未**承诺实现时间表）。**受众**：维护者与产品/协议设计者。  
**语言**：中文（暂无独立英文全译本；**`docs/en/DEVELOPMENT.md`** 架构节有英文指针）。  
**关联**：运行时行为以 **`docs/TOOLS.md`** 中 **`workflow_execute` / `workflow_validate`**、**`docs/DEVELOPMENT.md`** 中 **`agent::workflow`** 与 **`agent_turn` / 分阶段规划** 为准；本文定义**能力边界**与**推荐演进方向**，避免与现有 DAG 语义 silently 分叉。

---

## 1. 背景与问题

CrabMate 已具备：

- **轮内 DAG**：**`workflow_execute`**（`src/agent/workflow/`）——节点 **`deps`** 拓扑排序、层内并行、**`fail_fast`**、**`compensate_on_failure` / `compensate_with`**、节点级 **`max_retries`**（仅对部分**可重试**错误）、**`trace` / `workflow_run_id`** 与可选 Chrome Trace。
- **会话级多步**：**`agent_turn`** 外环（P/R/E）、**分阶段规划**（**`staged_plan_*`**）、**终答规划**（**`agent_reply_plan` v1**）与 **工作流反思**（**`workflow_reflection_controller`**、`workflow_node_id` 与 DAG 节点对齐规则等）。

用户与路线图期望更接近「**状态机式配置**」「**条件 / 循环的可读表达**」（对标常见 Agent 编排产品中的图 / StateGraph 体验）。  
**核心矛盾**：当前 **`workflow_execute` 的 `WorkflowSpec` 是有向无环图（DAG）**，**不**原生表达：

- 显式 **FSM**（命名状态 + 转移表）；
- 运行期 **二选一 / 多分支**（除「节点失败 → fail_fast / 补偿」外）；
- **真循环**（与拓扑排序、层调度、失败语义直接冲突）。

若在不澄清边界的情况下堆叠语法，易导致：**调度器复杂度爆炸**、**审批 / trace / 与 `agent_reply_plan` 对齐**语义断裂、以及**不可判定**的停机风险。

---

## 2. 设计目标（非功能列表）

| 目标 | 说明 |
|------|------|
| **可读** | 运维与作者能一眼看出「当前处于哪段业务状态、为何进入下一支」。 |
| **可执行** | 在单进程内可调度、可设超时与墙钟上限、可与现有 **工具审批**、**`tool_call_explain`**、**SSE** 对齐。 |
| **可观测** | 延续 **`workflow_run_id` / `trace` / `completion_order`**；新增概念须能映射到现有或扩展的 trace **事件类型**（避免「黑盒分支」）。 |
| **渐进兼容** | 默认保持现有 JSON **`workflow.nodes` + `deps`** 行为；新能力以 **可选字段 / 新 `kind` / 编译产物** 引入，**禁止** silent 改变旧 DAG 语义。 |

---

## 3. 现状：编排能力谱系（约定用语）

本节统一用语，便于后文「分层」讨论：

1. **轮内编排（Intra-turn）**  
   单次模型 **`tool_calls`** 触发的 **`workflow_execute`**：在 **`run_agent_turn` 的 E 步**内跑完；**`WorkflowNodeSpec`** 见 **`src/agent/workflow/model.rs`**，解析见 **`parse.rs`**，调度见 **`execute/schedule.rs`**。

2. **会话级编排（Inter-turn）**  
   多轮 **P → E → P…**：由 **`agent_turn::outer_loop`**、分阶段 **`staged`**、**`per_coord`** 等驱动；**不**等同于单次 DAG。

3. **声明式规划（Declarative plan）**  
   **`agent_reply_plan` v1**：步骤 **`id` / `workflow_node_id` / `executor_kind`** 等与 DAG 或 validate-only 结果对齐（规则见 **`plan_artifact`**、**`DEVELOPMENT.md`** 相关小节）。

**结论**：「状态机式」若指 **跨多轮、带业务语义的阶段**，更适合落在 **(2)+(3)** 或与 **(2)** 显式结合的「外层状态」；若强行塞进 **(1)** 且无界循环，会与 DAG 执行器假设冲突。

---

## 4. 目标概念与映射策略

### 4.1 状态机（FSM）

**推荐语义**：FSM 是 **「命名状态 + 带守卫的转移」** 的配置层抽象；**执行层**仍应落在以下之一：

- **A. 编译为 DAG（首选 MVP 方向）**  
  - 配置：`states`、`transitions`（`from`、`event` 或 `on`、`to`、`action`：`tool` + `args`）。  
  - 构建期或解析期展开为 **`WorkflowNodeSpec` 列表 + `deps`**（及可选「占位 / 同步栅栏」节点）。  
  - **优点**：复用 **`topo_layers`**、补偿、审批、trace、schema 粗校验全链路。  
  - **缺点**：**动态分支**（转移目标依赖运行时数据）需 **「先跑 guard 节点再选下一批边」**，编译器要生成 **choice 汇合** 模式。

- **B. 原生 FSM 执行器（远期）**  
  - 新 **`WorkflowKind::Fsm`** 或独立工具 **`workflow_execute_fsm`**，与 DAG **并列**，共享审批/trace 适配层。  
  - **优点**：表达动态转移更自然。  
  - **缺点**：两套调度语义、测试矩阵倍增；须严格定义与 **`workflow_execute_result`** 的 JSON 兼容策略。

**文档约定**：在实现落地前，对外若出现「状态机」一词，应标明是 **「编译到 DAG」** 还是 **「独立 FSM 引擎」**，避免与现有 **`workflow`** 混称。

### 4.2 条件（分支）

分层表达：

| 层级 | 手段 | 可读性手段 |
|------|------|------------|
| **DAG 内** | 前置只读节点（如 **`read_file` / `grep` / `run_command` dry-run**）+ **`deps`** 串行 | 节点 **`id` 命名规范**（`check_*` → `act_*`）；可选 **`display_name` / `doc`** 扩展字段（仅展示，不参与执行） |
| **DAG 内显式分支（未来）** | **`choice` 节点** 或 **`on_success` / `on_failure` 边**（概念） | 调度器根据**前驱节点结果**剪枝后续可运行集；**必须**写入 **trace**（含「未选分支 skipped」） |
| **跨轮** | 模型输出新 **`workflow`** JSON 或新 **`agent_reply_plan`** | 由 **会话级** 叙事承担，避免单轮 DAG 无限表达力 |

**守卫表达式**：不建议在首版引入任意表达式语言（安全与可测试性差）。推荐 **「工具即守卫」**：守卫逻辑封装在**只读工具**或**受控脚本工具**中，输出结构化 JSON，**`choice` 仅解析固定 schema**（如 `branch: "a"|"b"`）。

### 4.3 循环

**原则**：单轮 **DAG 不引入无界环**。可选安全子集：

1. **有界展开（`for_each` / `repeat N`）**  
   - 配置声明 **`max_items` / `max_iterations` 硬上限**；编译为多个节点或链式 **`deps`**。  
   - 适用于 CI 矩阵、多文件同构操作。

2. **外圈循环（Agent 级）**  
   - 「直到测试通过」由 **多轮 `run_agent_turn`** 或 **每轮重新生成较小 DAG** 完成；每轮 DAG 仍无环。  
   - 与 **`plan_rewrite` / 分阶段** 的「重试故事」一致，文档上易解释。

3. **禁止默认开放 `while(true)`**  
   - 若未来支持一般循环，须单独 **步数 / 墙钟 / Token** 三重上限，且 **trace** 可区分 **每次迭代**。

---

## 5. 与分阶段规划、工作流反思的关系

- **分阶段规划**：适合 **粗粒度阶段**（规划 → 只读审查 → 写补丁 → 测试），与 **`executor_kind`** 收窄工具集配合良好。  
- **`workflow_execute` DAG**：适合 **同轮内** 多工具并行、依赖、补偿。  
- **工作流反思 / `workflow_node_id`**：保证 **规划文本** 与 **最近一次 DAG 节点集合** 可对齐；若引入 FSM 编译，**编译后的 `nodes[].id`** 仍须满足现有对齐与校验规则，或 **单独定义「逻辑节点 id → 物理节点 id」映射** 并在文档中说明。

**推荐产品叙事**：

- **「阶段 / 角色 / 策略」** → 分阶段 + PER。  
- **「同轮工具链 / CI 流水线片段」** → DAG（或 FSM→DAG）。  
- **「长程目标分解」** → `agent_reply_plan` + 多轮。

---

## 6. 可观测性与契约

任何扩展须回答：

1. **`workflow_execute_result`**（或并列的 **`workflow_fsm_result`**）是否 **JSON 形状兼容**（至少 **`workflow_run_id` + `status` + `trace`** 存在）？  
2. **审批**：动态分支是否导致「未运行节点已预审批」？若会，须定义 **惰性审批** 或 **分支级审批键**。  
3. **Chrome Trace / 回合 Trace 合并**：与 **`request_chrome_trace`** 的合并规则是否仍适用（见 **`DEVELOPMENT.md`**）？  
4. **工作区变更集**：DAG 内工具仍走独立 **`ToolContext`**，**不**写入 **`workspace_changelist`**——FSM 路径若引入新执行器，须 **显式继承或放弃**该语义并在 **`TOOLS.md`** 写明。

---

## 7. 演进路径（建议）

| 阶段 | 内容 | 产出 |
|------|------|------|
| **Phase 0** | 文档与示例：DAG **`id` 命名规范**、典型「条件链」模板、外圈循环示例 | **`docs/TOOLS.md` 片段 + 本设计稿修订** |
| **Phase 1** | **配置态 FSM → 编译为 `WorkflowSpec`**（无运行期动态转移，或仅允许常量分支表） | 新模块如 **`agent/workflow/compile_fsm.rs`** 或 CLI **`crabmate workflow compile`**；单测覆盖展开与上限 |
| **Phase 2** | **`choice` 节点** + trace 剪枝语义 | 扩展 **`parse.rs` / `execute/schedule.rs`**；金样 JSON fixture |
| **Phase 3** | **动态守卫 + 有界循环** + 与审批的惰性策略 | 设计评审 + 安全评审 |

**非目标（当前共识）**：在 **`workflow_execute` 单入口** 内同时支持 **任意图灵完备脚本** 与 **无界循环**，且不增加独立资源上限。

---

## 8. 相关源码索引（实现时从这里读）

| 区域 | 路径 | 说明 |
|------|------|------|
| DAG 规格 | `src/agent/workflow/model.rs` | **`WorkflowSpec` / `WorkflowNodeSpec`** |
| 解析 | `src/agent/workflow/parse.rs` | **`parse_workflow_spec`**、`nodes` 数组/对象 |
| 拓扑 | `src/agent/workflow/dag.rs` | **`topo_layers`** |
| 调度与执行 | `src/agent/workflow/execute/` | **`schedule` / `node` / `retry` / `compensation`** |
| 工具入口 | `src/agent/workflow_tool_dispatch.rs` | **`dispatch_workflow_execute_tool`** |
| Schema 粗校验 | `src/tools/schema_check.rs` | **`workflow_tool_args_satisfy_required`** |
| 规划对齐 | `src/agent/plan_artifact.rs` 等 | **`workflow_node_id`、validate-only 绑定** |

---

## 9. 修订记录

| 日期 | 摘要 |
|------|------|
| 2026-04-12 | 初稿：分层（轮内 DAG / 会话级 / 规划）、FSM 编译优先、条件/循环边界与非目标。 |
