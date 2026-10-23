# 多 Agent 协作：分层演进设计草案

**状态**：设计稿（**未**承诺实现时间表；默认**不**进入当期迭代承诺）。  
**受众**：维护者、产品与协议设计者。  
**语言**：中文。  
**维护**：实现或废弃某能力时，同步修订本文，并更新 **`docs/未来规划功能.md`** / **`docs/en/FUTURE_PLANS.md`** 中的索引段落。  
**本次修订（2026-10）**：依据一轮架构评审（通用性 / 拓展性 / 任务粒度），强化「契约不承载内部表示」「粒度先于委托」「验收必须可判定」「权限裁决链唯一」四处约束，并把评审结论前置到 §0。

**关联文档**：

- **`docs/design/archive/分层多智能体架构.md`**：历史 Manager + Operator 设计，**仅供考古**；其入口（`hierarchy` / `staged`）已整棵移除，勿据其做实现索引。
- **`docs/design/audience_critic_role.md`**：侧向点评（本路线 **L1** 的既有设计，锚点 C / D）。
- **`docs/design/run_loop_state_ownership.md`**：回合可变状态归属；委托**不**新开并行阶段机。
- **`docs/design/per_state_machine_consolidation.md`**：P/R/E 状态机与反射控制器的职责边界。
- **`docs/工作流编排架构.md`**、**`src/cm_workflow/model.rs`**：轮内 DAG（`deps` 拓扑分层 / 并行 / `fail_fast` / 补偿 / `max_retries` / `for_each` / `run_if`）。
- **`docs/SSE协议.md`**：新增控制面事件须与 **`src/cm_sse_protocol`**、golden fixtures、**`scripts/check-sse-protocol.sh`** 同步。
- **`docs/配置说明.md`**：新增配置键的落点与热重载语义。
- **`src/agent/agent_turn/host/params.rs`**：既有 `planner_model` / `executor_model` + `use_executor_model` 的按阶段换模型机制（§5 的 `model_hint` 与 §0-6 的基础）。

---

## 0. 关键决策与已知风险（强调）

以下 6 条是本设计**最容易被实现者走偏**之处；契约字段一旦落地，返工成本随时间上升，故前置强调：

| # | 强调点 | 结论 |
|---|---|---|
| 1 | **契约不承载内部表示** | `context_slice` 是**结构化 DTO**，**不是** `messages`。`messages` 是后端内部表示（MiniMax 网关会把 `system` 合入 `user`），跨后端 / 跨进程不稳定；渲染成 `messages` 是**子 Agent 侧**的责任。 |
| 2 | **粒度先于委托** | 先过 **§6.6 门限**，不满足就自己做。委托只对「预期 ≥3 次工具调用 / 需上下文隔离 / 可并行」的任务；单次读文件、单次检索**禁止**委托。 |
| 3 | **验收必须可判定** | `Task.acceptance` + 既有 **`step_verifier`**；`Result.status == ok` **不**由子 Agent 自述决定，自述仅为参考信号。 |
| 4 | **权限裁决链唯一** | 按 **§6.5** 的固定顺序取交。`allowed_tools`（**名字集合**）与 `node_tool_role`（**类别枚举**）粒度不同，**不可互相替代**。 |
| 5 | **上限要算组合账** | L3 层内并行与 `multi_agent_max_concurrent_children` **取 min**，另设**全局总闸**；差异化走「全局默认 + 角色覆盖」，**不再堆扁平键**。 |
| 6 | **模型选择是收益来源** | 复用既有 `planner_model` / `executor_model` 按阶段换模型机制，由 `Task.model_hint` 表达。缺这一维，多 Agent 只剩成本、没有收益。 |

**已知风险（接受并显式记录，不以「以后再说」掩盖）**：

- 预算**不含工具调用次数**（[turn_budget.rs](file:///home/gzz/crabmate/server/src/cm_agent/turn_budget.rs#L95-L142) 仅 `llm_calls` / `estimated_tokens` / 墙钟），首版**以墙钟兜底**。
- 与 A2A 的映射**有损**：首版不支持 `auth-required`、推送通知、流式状态事件（§8）。
- 长任务**不能**转后台：与既有 `background_tool_jobs` 的关系尚未定，首版不做（§8）。

---

## 1. 结论摘要

1. **A2A 不是多 Agent 协作的前置条件**。它属**跨进程互操作层**，排在本路线最后；先做单进程内的**有界委托**，再谈独立实例与跨进程协议。
2. 建议路线为 **L0 逻辑角色 → L1 有界侧向调用 → L2 单轮内有界子 Agent 委托 → L3 workflow agent 节点 → L4 跨进程 → A2A**，每层独立可交付、可回退。
3. **L2 是 MVP 起点**，也是唯一需要真正新增运行时的层：独立 `messages` + 工具子集 + **共享预算** + 结构化 Artifact + `trace_id` + **默认关闭** + 深度 / 并发硬上限 + 可整体降级回单 Agent。
4. **第一步（A2A 与内部多 Agent 唯一共用物）**：定义 **Agent 调用契约**（§5 的 `Task` / `Result`）。内部委托与将来的 A2A `task` 共用同一份语义，避免两套模型。
5. **契约必须先于实现定型且保持后端无关**：粒度门限（§6.6）、权限裁决链（§6.5）、验收判据（`acceptance`）三件事**写在契约里**，否则实现者会各自发明一套——这是 §0 强调的核心。

---

## 2. 现状盘点与历史教训

| 层 | 现状 | 位置 | 缺口 |
|---|---|---|---|
| **L0 角色** | **已有**：人设 + 工具白名单（`allowed_tools`），按回合 / 会话切换，同一时刻仅一个生效；`act` 模式工具 = 全量 ∩ 角色白名单，越权调用在执行层被拒 | `src/cm_config/agent_role_spec.rs`、`src/cm_internal/agent_role_turn.rs`、`src/cm_types/session_mode.rs`、`config/agent_roles.toml` | 无并发角色；无角色间消息 |
| **L1 侧向调用** | **设计就绪**：观众 / critic，无工具、有界、默认 **fail-open**，锚点仅 C（终答规划闸门旁）与 D（`WorkflowReflectionController::decide` 后） | `docs/design/audience_critic_role.md` | 未实现；不产出可消费工件 |
| **轮内 DAG** | **已有**：`workflow_execute` 的 `deps` 拓扑分层（Kahn）、层内并行、`max_parallelism`、`fail_fast`、`compensate_on_failure` / `compensate_with`、节点 `max_retries`、`for_each`、`run_if`、`workflow_run_id` | `src/cm_workflow/` | **节点是工具节点**（`tool_name` + `tool_args`），**不能**承载 Agent |
| **跨轮编排** | **已有**：ReAct 外循环 + `plan_artifact` + `workflow_reflection_controller` | `src/cm_agent/`（外循环） | 同一时刻仍是单一主 Agent |
| **MCP** | **已有**：客户端支持 stdio 与 Streamable HTTP；服务端 `crabmate mcp serve` 支持 stdio / TCP，**无传输层鉴权** | `src/cm_mcp/` | 服务端缺鉴权；与 Agent↔Agent 正交 |
| **历史分层多 Agent** | **已移除**：Manager / Operator / Router（`src/agent/hierarchy/`）整棵删除并归档 | `docs/design/archive/分层多智能体架构.md` | — |

**教训**：上一版分层方案在**单进程内**引入了完整的 Manager/Operator 阶段机与专用状态，最终因复杂度与收益不匹配被整棵移除。新方案因此**不新开阶段机**、**不共享可变 `messages`**、**默认关闭**，把「一个主循环 + 有界委托」作为不可动摇的基线。

---

## 3. 术语与范围

| 术语 | 含义 |
|---|---|
| **角色（L0）** | 会话主 Agent 的**人设 + 工具白名单**，按回合切换。**不是**独立实例。 |
| **侧向点评（L1）** | 编排层发起的**额外无工具** `chat/completions`，输入为服务端构造摘要，输出结构化点评。见 `audience_critic_role.md`。 |
| **子 Agent / 委托（L2）** | 父 Agent 在**单轮内**发起的一次**有界** Agent 调用：独立 `messages`、工具子集、共享 `TurnBudgetCounter`。 |
| **agent 节点（L3）** | `workflow_execute` DAG 中**新增的节点类型**，其执行体是一次委托。 |
| **A2A** | Agent↔Agent 的**跨进程**互操作协议（Agent Card / Task 生命周期），走网络。 |
| **MCP** | Agent↔**工具**协议，与本设计正交。 |

**不在本设计范围内**：多进程独立「评委服务」、无上限的逐层嵌套、Agent 之间共享可写工作区状态、自动改写模型权重。

---

## 4. 分层演进路线

| 层 | 能力 | 前置条件 | 新增运行时 |
|---|---|---|---|
| **L0** | 逻辑角色（人设 + 工具白名单） | —（**已完成**） | 无 |
| **L1** | 有界侧向点评 | L0 | 无（复用 `llm::complete_chat_retrying`，挂现有锚点） |
| **L2** | 单轮内有界子 Agent 委托 | L1 的预算 / 摘要 / 脱敏基建 | **有**：委托器 + 独立 `messages` 会话 + 子预算视图 |
| **L3** | workflow agent 节点 | L2 + `cm_workflow` 新增节点类型 | 复用现有 DAG 调度，仅扩节点枚举 |
| **L4** | 跨进程独立实例 | L3 + MCP 服务端鉴权 / 传输加固 | 有：进程间任务与结果传输 |
| **A2A** | 对外 Agent 互操作 | L4 | 有：Agent Card、Task 生命周期映射 |

**排序理由**：每一层都以前一层已稳定的**预算、摘要、脱敏、可观测性**为地基；A2A 在缺少 L2/L4 时只能得到一个"没有可协作对象"的空协议层。

---

## 5. Agent 调用契约（`Task` / `Result`）

内部委托（L2/L3）与将来的 A2A `task` **共用同一份语义**；A2A 层只做字段映射与传输包装。

**`Task`（父 → 子）**

| 字段 | 类型 | 说明 |
|---|---|---|
| `protocol_version` | `string` | 契约版本（如 `"1"`）。**跨进程 / 外部 Agent 接入的第一手判据**，缺它则 A2A 映射无从对齐。 |
| `task_id` | `string` | 本次委托唯一 id（服务端生成）。 |
| `root_turn_id` | `string` | 根回合 id。与 `parent_trace_id` 并存：长链归并整棵树时**无需逐级回溯**。 |
| `parent_trace_id` | `string` | 直接父回合 / 节点，供 span 串联。 |
| `goal` | `string` | 单句目标。**只描述要什么，不承担验收判据**（判据见 `acceptance`）。 |
| `acceptance` | `string[]` | **可判定的验收条目**。复用分阶段 `steps[].acceptance` + `step_verifier` 的既有语义，**不新造判定器**；`Result.status == ok` 由此产出（§0-3）。 |
| `context_slice` | `ContextSlice` | **结构化 DTO**：`{ summary, facts[], files[], input_artifacts[] }`。**不得**是 `messages` —— `messages` 是后端内部表示（MiniMax 网关会把 `system` 合入 `user`），跨后端 / 跨进程不稳定；渲染成 `messages` 是**子 Agent 侧**的责任（§0-1）。 |
| `role_id` | `string?` | 子 Agent 人设；缺省继承父角色的人设、**不**继承其工具全量。 |
| `model_hint` | `string?` | 期望模型档位 / 名称。项目已有按阶段换模型机制（首轮 `planner_model`、其后 `executor_model`，见 `agent_turn/host/params.rs` 的 `use_executor_model`）；本字段是其向「按 Agent 选模型」的自然延伸（§0-6）。 |
| `tool_allowlist` | `string[]` | **请求**的工具集。**有效权限**按 **§6.5 裁决链**取交，**不是**「父白名单真子集」这一条就能裁定；实现层固定剔除 `workflow_execute`（防递归）与委托工具本身（防重入）。 |
| `budget` | `{ wall_clock_secs, max_llm_calls, max_tokens }` | 子任务视图；不得超过父剩余额度。**首版不含工具调用次数**（§6.4），由墙钟兜底。 |
| `depth` | `u32` | 当前嵌套深度，> 上限直接拒绝。 |
| `expected_output` | `string?` | 期望工件类型，取值受 `artifacts[].kind` 词汇表约束（**非**自由字符串）。 |

**`Result`（子 → 父）**

| 字段 | 类型 | 说明 |
|---|---|---|
| `task_id` | `string` | 回填。 |
| `status` | `ok \| partial \| failed \| denied \| cancelled \| timeout \| budget_exceeded \| input_required` | `denied` 为策略 / 权限拒绝；`timeout` / `budget_exceeded` 复用 `TurnBudgetDeny` 三态语义；`cancelled` 为用户 / 父主动中断；`input_required` 为子 Agent 需回溯父或人补充输入。**后两者是对齐 A2A 状态集补上的**：缺它们，§8 的「字段级映射」不成立。 |
| `artifacts` | `[{ kind, summary, uri? }]` | **结构化**。`kind` 取受控词汇表（`patch` / `report` / `review` / `answer`），**不是自由字符串**（否则跨进程无法互操作）；`summary` 有长度上限（复用 `output_inject_max_chars`），**超过门限的正文必须落 `uri`，不内联回灌父上下文**。 |
| `usage` | `{ llm_calls, estimated_tokens, wall_clock_secs }` | 已计入父 `TurnBudgetCounter` 的份额；取委托前后**快照差值**（计数器只有累加与读取，**无** drain / 重置语义）。 |
| `trace_id` | `string` | 与 `parent_trace_id` 串联。 |
| `deny_reason` | `string?` | 复用 `TurnBudgetDeny` 语义（`wall_clock` / `llm_calls` / `tokens`）。 |

```json
{
  "protocol_version": "1",
  "task_id": "t-8f3c",
  "root_turn_id": "turn-2026-10-17-001",
  "parent_trace_id": "turn-2026-10-17-001/n-2",
  "goal": "审查 src/workspace/path.rs 的符号链接处理",
  "acceptance": ["结论包含符号链接逃逸风险判定", "列出涉及的具体函数名"],
  "role_id": "code_reviewer",
  "model_hint": "executor",
  "context_slice": {
    "summary": "父正在为工作区路径加固收集风险点，已确认 openat2 已贯通读写删。",
    "facts": ["path.rs 使用 openat2 + RESOLVE_IN_ROOT"],
    "files": ["src/workspace/path.rs"],
    "input_artifacts": []
  },
  "tool_allowlist": ["read_file", "grep"],
  "budget": { "wall_clock_secs": 120, "max_llm_calls": 6, "max_tokens": 20000 },
  "depth": 1,
  "expected_output": "review"
}
```

---

## 6. L2/L3 落地要点：开关、上限、降级与跨层约束

### 6.1 开关（默认全关）

| 配置键（建议） | 默认 | 说明 |
|---|---|---|
| `multi_agent_enabled` | `false` | 总开关；关闭时委托工具**不注册**、相关 SSE 事件**不发送**。 |
| `multi_agent_delegate_tool_enabled` | `false` | 单独暴露委托工具的开关（总开关开启后才生效）。 |
| `multi_agent_role_ids` | `[]` | 允许被委托的角色白名单；空 = 不启用。 |

### 6.2 上限（硬上限不可被请求体突破）

| 配置键（建议） | 建议默认 | 硬上限 | 说明 |
|---|---|---|---|
| `multi_agent_max_depth` | `1` | `2` | 超出即 `denied`。 |
| `multi_agent_max_concurrent_children` | `2` | `4` | 同时存活的子 Agent 数。 |
| `multi_agent_max_children_per_turn` | `4` | `8` | 单回合累计委托次数。 |
| `multi_agent_child_timeout_seconds` | `120` | — | 单个子任务墙钟上限。 |
| `multi_agent_child_max_llm_calls` | `6` | — | 单子任务 LLM 次数；且 `min(自身, 父剩余)`。 |

**差异化只走两层，不再堆扁平键**：上表是**全局默认**；允许按角色覆盖（`multi_agent_role_limits[role_id] = { max_depth, max_concurrent_children, ... }`）。**不要**为每个角色新增一组扁平全局键——那会让组合数随角色数线性膨胀。

**与 DAG 并行必须显式取组合，不能各算各的**：L3 的层内并行由 `WorkflowSpec.max_parallelism` 控制，与 `multi_agent_max_concurrent_children` **取 `min`**；此外另设一个**全局同时存活子 Agent 总上限**作为总闸（跨层、跨 DAG 生效），防止 `depth × 层数 × 层内并行` 相乘突破单键上限。

### 6.3 降级（fail-open，可整体回退）

- 子任务失败 / 超时 / 预算拒绝：**不阻断**父回合，父收到 `status != ok` 的 `Result` 并自行决定是否重规划。
- 触发 `TurnBudgetDeny`（`wall_clock` / `llm_calls` / `tokens`）：父进入**已有的** `degradation_active` 路径（`src/cm_agent/turn_budget.rs`），**不**新增退化分支。
- 全局熔断：`multi_agent_enabled=false` 必须能在**不重启**的情况下把行为收敛回 L0/L1（配置热重载，见 `docs/配置说明.md`）。

### 6.4 状态与可观测性

- **预算**：子 Agent **共用**父的 `TurnBudgetCounter`（`Arc` 共享；`llm_calls` / `estimated_tokens` 原子累加）。`Result.usage` 取委托**前后快照的差值**；计数器只有累加与读取，**没有** drain / 重置语义，**不要**按「各 drain 一次份额」实现。
- **预算维度缺口**：现有计数仅 `llm_calls` / `estimated_tokens` / 墙钟，**不含工具调用次数**。而子 Agent 的主要扇出消耗恰恰是工具调用，首版**显式以墙钟兜底**，并把「新增工具调用计数维」列为后续项，而不是默认它已被覆盖。
- **状态归属**：子 Agent 的 `messages` 为**独立会话对象**，生命周期不越过单次委托；父的可变 `messages` 由 `run_loop_state_ownership.md` 的既有规则拥有。
- **可观测**：委托开始 / 结束 / 拒绝以 tracing span + 可选 SSE 事件暴露；若新增 SSE 事件，**必须**走 `docs/SSE协议.md` 契约与 golden fixtures。
- **安全**：子 Agent 请求与日志遵守脱敏 / 截断（`AGENTS.md` → Repository engineering rules → Secrets & logging）；写类工具默认**不**下放给子 Agent，如需下放须经 `tool_approval` 并单独文档化风险。

### 6.5 工具权限裁决链（唯一顺序，不可各自发明）

系统里**已并存三套**工具约束机制（`allowed_tools` / `node_tool_role` / `tool_allowlist`），各管一段、粒度不同；下表把它展开成**四段裁决顺序**，实现时**必须**按此顺序取交，否则实现者会各自发明一套：

| 顺序 | 机制 | 形态 | 作用 |
|---|---|---|---|
| 1 | `AgentRoleSpec.allowed_tools` | **名字集合**（`Option<Arc<HashSet<String>>>`，`"mcp"` 通配） | 角色的最终权限上界 |
| 2 | `WorkflowNodeToolRole`（`node_tool_role`，仅 L3） | **类别枚举**（`ReviewReadonly` / `PatchWrite` / `TestRunner`） | 节点的**粗粒度收窄** |
| 3 | 类别 `node_tool_role` 的落地策略（`step_executor_policy.rs` + `PlanStepExecutorKind` 策略表） | 策略函数 | 把第 2 段的类别解析为具体工具行为 |
| 4 | `Task.tool_allowlist` | **请求子集**（`string[]`） | 父对单次委托的进一步收窄 |

**裁决规则**：有效工具集 = 上述四者的**交集**，再**固定剔除** `workflow_execute`（防递归）与委托工具本身（防重入）。

**两条不可混用的性质**：
- `allowed_tools`（**名字集合**）与 `node_tool_role`（**类别枚举**）**粒度不同，不可互相替代**；前者是**最终裁决**，后者只用于**粗粒度收窄**（L3 无 `node_tool_role` 时该项不参与）。
- `Task.tool_allowlist` **只能是交集的下界约束**，**不是**「父白名单真子集」——把权限来源钉死在父角色会挡住「父受限、子角色更宽」的合法诉求，也会让 A2A 侧无法对齐。

### 6.6 委托粒度门限（何时才**该**委托）

粒度判据此前完全缺失，导致「该不该委托」无标准。门限如下，**先过期门限再委托**（§0-2）：

**应委托**（满足任一）：
- 预期 **≥3 次工具调用**的独立子任务；
- 需要**上下文隔离**（子任务的中间过程会污染父上下文）；
- 可与其他子任务**并行**。

**不应委托**（反例，命中即自己做）：
- 单次读文件、单次检索（委托的构造 + 往返成本高于收益）；
- 需要**未摘要的长正文**才能决策（切片会把关键信息截掉）；
- 与父步骤**强顺序耦合**（子任务结论必须逐字回到父的下一步）。

**验收归属**：`Result.status == ok` 由 `Task.acceptance` + 既有 `step_verifier` 产出，**不由子 Agent 自述**（§0-3）；自述仅作参考信号。

---

## 7. L3：workflow agent 节点

**现状明确**：`WorkflowNodeSpec` 的节点是**工具节点**（`tool_name` + `tool_args` + `deps` / `requires_approval` / `timeout_secs` / `compensate_with` / `max_retries` / `node_tool_role` / `run_if`），**没有** agent 节点类型。因此 L3 是一个**新增节点类型**的设计决策，而非复用既有字段。

**设计要点**：

1. **只扩节点类型，不新开调度器**：agent 节点的执行体是一次 L2 委托，`deps` 分层、层内并行、`max_parallelism`、`fail_fast`、`compensate_with`、`max_retries` 全部沿用现有实现。
2. **工具子集沿用现有词汇表**：`node_tool_role` 现有枚举 `WorkflowNodeToolRole::{ReviewReadonly, PatchWrite, TestRunner}`（与 `PlanStepExecutorKind` 共用策略表），可作为 agent 节点**粗粒度**工具约束；需要更细粒度时用 `Task.tool_allowlist`，且必须与 `AgentRoleSpec.allowed_tools` 取交。
3. **agent 节点不可再展开 workflow**：与 L2 同一条防递归规则（剔除 `workflow_execute`）。
4. **补偿语义**：agent 节点失败走现有补偿链；补偿动作只允许**工具节点**或 `ReviewReadonly` 类 agent 节点。
5. **节点上下文装配规则（此前缺失，必须补齐）**：agent 节点沿用工具节点的数据流——`goal` 来自节点**声明模板**；`context_slice` 由**上游节点输出 + 声明模板**装配，并遵守 `output_inject_max_chars` / `summary_preview_max_chars` 门限。**禁止** agent 节点绕过通道自取全局状态，否则「只读切片」的硬约束（§9-2）在 L3 失效。

---

## 8. A2A 定位与分期

A2A 解决的是**跨进程、跨组织的 Agent 互操作**，与本设计的 L0–L3 正交；它**不**提供子 Agent 的预算、状态、工具子集能力，因此不能替代 L2。

| A2A 概念 | 本设计对应物 | 说明 |
|---|---|---|
| **Agent Card** | 角色元数据 + 能力声明 | L0 已有角色，L4 再补传输端点与鉴权声明。 |
| **Task 生命周期** | §5 的 `Task` / `Result` | 字段级映射，尽早统一语义即可，无须等待 L4。 |
| **Artifact** | `Result.artifacts` | 复用同一结构，避免两套工件模型。 |
| **Transport** | L4 进程间传输 | 优先走 MCP 服务端加固后的通道（补 **传输层鉴权**），再考虑独立 A2A 端点。 |

**映射是有损的（显式声明，不假装等价）**：`Result.status` 已按 A2A 状态集补齐（§5 的八态含 `cancelled` / `input_required`），但 A2A 的 **`auth-required`**、**推送通知**、**流式 `TaskStatusUpdateEvent`** 首版**不支持**——它们依赖传输层与会话模型，属 L4 议题。首版只做**字段级映射**，遇到未支持的状态一律降级为 `failed` + `deny_reason`。

**与 `background_tool_jobs` 的关系（显式表态）**：现有后台任务作业是**工具级**的长任务，**不**等价于子 Agent。首版**不**把长委托转后台，委托生命周期仍**严格限于单轮**（§9-1）；「长任务转后台」列入 L4 之后单独评估，避免把两个生命周期模型混为一谈。

**分期**：L2 定型 → L3 验证 DAG 内委托 → L4 进程间（先补 MCP 服务端鉴权）→ A2A（仅做映射与对外端点，**不**回改内部契约）。

---

## 9. 质量属性硬约束（不可协商）

1. **不新开并行阶段机**：编排仍是 `session_mode →（Act 句启发式）→ ReAct 外循环` + 既有 DAG。
2. **不共享可变 `messages`**：委托的上下文是父构造的**只读切片**。
3. **预算单点**：所有额外 LLM 调用（L1 侧向、L2 子 Agent、L3 节点）计入同一个 `TurnBudgetCounter`；不得绕过 `TurnBudgetDeny` 三态。
4. **递归与扇出硬上限**：深度、并发、每回合次数均有编译期常量级硬上限，请求体不可突破。
5. **默认关闭 + fail-open + 可整体降级**：任一层出问题都能退回 L0/L1。
6. **外部契约先行**：新增 SSE 事件 / API 字段走 `docs/SSE协议.md` 与 `docs/命令行与路由.md`，并同步 golden fixtures 与 `scripts/check-sse-protocol.sh`。
7. **工具越权在实现层拒绝**，而不只靠提示词约束（沿用 `agent_role_turn.rs` 的既有做法）。
8. **文档同步**：改动模块布局 / 分层时更新 `docs/开发文档.md`（见 `AGENTS.md` → Repository engineering rules → Architecture & documentation sync）。
9. **契约不承载内部表示**：`context_slice` 是结构化 DTO，**不得**是 `messages`；渲染成 `messages` 是子 Agent 侧的责任（§0-1、§5）。
10. **粒度先于委托**：委托前必须过 §6.6 门限；单次读文件 / 单次检索**禁止**委托。
11. **权限裁决链唯一**：所有工具授权按 §6.5 的固定顺序取交，**不允许**在此之外另设一条授权路径。

---

## 10. 建议分期（仅为排序建议，非时间承诺）

| 阶段 | 交付物 | 验收要点 |
|---|---|---|
| **P1** | L1 侧向点评（按 `audience_critic_role.md`） | 默认关闭；失败不阻断主循环；预算计入共享计数器 |
| **P2** | §5 契约定型 + L2 委托 MVP（默认关闭） | 深度 / 并发 / 次数上限生效；`deny_reason` 可观测；降级回单 Agent 有测试；**§6.5 裁决链与 §6.6 粒度门限均有测试**（工具授权无旁路、单次读文件不委托） |
| **P3** | L3 agent 节点 | 复用 DAG 调度；`fail_fast` / `max_retries` / 补偿行为与工具节点一致 |
| **P4** | L4 跨进程（先补 MCP 服务端鉴权） | 传输鉴权就绪；任务 / 结果传输可观测 |
| **P5** | A2A 映射 | 仅做字段映射与对外端点，不改内部契约 |
