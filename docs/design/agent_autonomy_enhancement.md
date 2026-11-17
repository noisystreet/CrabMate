# 设计文档：Agent 自主能力增强路线

**状态**：设计草案（待评审；评审结论回填 §5 与 §8 后再排期）。**语言**：中文。

**跟踪**：[#1386](https://github.com/noisystreet/CrabMate/issues/1386)（路线级共识与各阶段进度）。
**前置项 S0.5**：[#1387](https://github.com/noisystreet/CrabMate/issues/1387)（完成判定层重建）。
实现前先按 `AGENTS.md` §5「先有 Issue」为各阶段单独建子 Issue。

**关联文档**：

- [`subagent_delegation.md`](./subagent_delegation.md)：隔离上下文的子 Agent 委托（#839）。**设计定稿、P0 前置**
  **重构已落地、P1 功能未开始**。
- [`multi_agent_collaboration.md`](./multi_agent_collaboration.md)：多 Agent 分层演进 L0–L4 与 A2A 定位；
  委托粒度门限见该文 §6.6。
- [`agent_self_evolution_roadmap.md`](./agent_self_evolution_roadmap.md)：记忆与经验沉淀的进化闭环（A/B/C/D/E 五层）。
- [`permission_levels.md`](./permission_levels.md)：L0–L4 权限等级、六轴投影与等级作用域审批键。
- [`run_loop_state_ownership.md`](./run_loop_state_ownership.md)：回合可变状态归属（本文字段落点须与之对齐）。
- [`per_state_machine_consolidation.md`](./per_state_machine_consolidation.md)：P/R/E 状态机与反射控制器边界。
- [`../待办清单.md`](../待办清单.md)：多 Agent 协作、长期记忆等未完成条目（本文不复制其内容）。

---

## 1. 目标与非目标

### 1.1 目标

提升 Server Agent **在无人值守或少人值守下把一件事做完**的能力。具体指三条可验证的性质：

1. **续航**：任务不必每轮由用户再发一句话才能推进。
2. **自证**：是否完成由**可判定证据**（退出码、文件、输出、HTTP 状态）说了算，而非模型自述。
3. **可隔离**：探索与改动的上下文不互相污染，失败可整体丢弃。

### 1.2 非目标

- **不**新增 `permission_level` 档位来「表达更自主」。六轴投影与等级作用域审批键（`l3|git`）已定型，
  另开一档等于造第二套权限语义，与 `AGENTS.md` §4.6 的边界口径冲突。
- **不**恢复全局单表 FSM、`hierarchy/` / `staged/` 分层入口、L2 意图分类。三者均已归档并移除，
  重启会被 `deny_unknown_fields` 与既有金样拦下（见 `docs/待办清单.md`「编排层改进」小节）。
- **不**把 LLM 凭据（`api_key` / `client`）下沉到工具执行层以换取「工具里能开子回合」。
  该分层是 `docs/design/subagent_delegation.md` §3 的显式否决项。
- **不**做模型权重训练 / 微调；本仓库只对接 OpenAI 兼容 `chat/completions`。
- **不**用「继续补关键词」的方式修完成判定。综合任务无单一 `exit_code` 可代表完成，关键词范式已被
  §3.2 实测证伪（同一事实因工具名不同而判定相反）。判定层改走「结构化证据 + LLM 语义判定」（§4 L1）。
- **不**让 LLM 判定无锚点下结论：`satisfied` 必须附非空 `evidence_refs`，且输入不含 assistant 自述正文。

---

## 2. 现状基线（代码事实）

本节只记录**已核对的**现状，供评审时不必重复考古。

| 能力面 | 现状 | 锚点 |
|---|---|---|
| 编排 | 只有 ReAct 一条路；`TurnRouteDriver` 仅 `ReAct` 一个变体 | `src/agent/agent_turn/policy/turn_route_decision.rs` |
| 路由 | Act 回合起点的**执行约束语义判定**（一次无工具短请求，可挂只读收窄）；Ask/Plan 跳过 | `TurnStartSnapshot::ActHeuristics` |
| 预算 | 墙钟 / Token 粗估 / LLM 次数 + 迭代硬上限；耗尽即报错中止 | `src/cm_agent/turn_budget.rs`、`turn_loop/outer_loop.rs` |
| 上下文 | 分级压缩：压旧工具正文 → 删最旧完整交互组，保工具链完整 | `src/agent/context_compaction/policy.rs` |
| 验收内核 | **无生产消费者**：`verify_against_spec` 等只剩 golden/单测（`staged-removal` 后）；仅 `apply_executor_kind_acceptance_defaults`、`is_effective` 在用 | `src/cm_agent/acceptance/` |
| 完成证据（主路径） | **已接线**：`outer_loop` → `turn_early_stop_allowed_with_judge`（第 ② 层语义判定）；冗余工具抑制读 `PerCoordinator` 缓存的同一结论，两道门同源。**非**「靠模型自述」 | `turn_loop/turn_completion.rs`、`turn_loop/outer_loop.rs` |
| 证据检测器覆盖 | **检测器已整层删除**（#1415）：第 ① 层不再自下结论，只剩 `evidence.rs` 的结构化事实采集供 judge 消费 | `policy/task_level_evidence/evidence.rs` |
| 早停门 | 只有一个判定源：第 ② 层语义判定（`completion_judge_enabled`）；判定不可用即不早停，**无确定性短路** | `policy/turn_completion_decision.rs`、`turn_loop/turn_completion.rs` |
| 步后抑规划 | `evaluate_turn_suppress_replanning` **注释自述生产未接线**（仅金样与单测） | `turn_completion.rs`、`turn_completion_decision.rs` |
| 子 Agent | `HandlerId::Delegate` 未注册；agent 层 handler 清单只有 `Workflow` | `host/execute/tool_execution_host.rs` |
| 跨回合状态 | plan artifact 只在回合内；跨回合仅 `reminder` / `event` 与 cron 定时任务 | `src/cm_config/scheduled_agent_task.rs` |
| 记忆 | scope **固定 `conversation_id`**；自动沉淀只覆盖 build/test 失败→成功 | `config/memory.toml`、`auto_memory` 系 |
| 工具续航 | 后台 job（`async=true`）、只读并行批、失败透明重试（**默认关**） | `TurnBudgetConfig` 同文件工具段 |

**一句话概括**：门禁强、续航弱、判定无确定性兜底。安全边界与预算约束做得扎实；完成证据的**机制已接线**
（第 ② 层语义判定 + 结构化事实输入，#1415 后检测器与关键词路径均已删除），但**早停现在完全依赖 LLM 可用性**，
「做完一件事」的半径仍被压在一轮内。

---

## 3. 瓶颈分析

按「是否卡住自主闭环」排序。

### 3.1 B1 任务状态无法跨回合存活（结构性阻塞，收益最高）

预算耗尽或需要用户输入时，回合以 `RunAgentTurnError` 终止，进度**不落盘**。用户必须再发一句话才能推进，
而模型下一回合只能从压缩后的对话里重新推断「做到哪了、还差什么」。

后果：长任务的边际成本随轮数上升，且每次重启都可能重复已完成的工作。

### 3.2 B2 完成证据机制已接线，但判不出综合任务，且双向误判

**先纠正一个易误判的点（本文早期草稿曾在此出错）**：主 ReAct 路径**并非**「靠模型自述」。工具批结束后
`outer_loop.rs` 已调用 `turn_early_stop_allowed`，经 `check_active_user_goal_completion_evidence` 做证据核对。

**实测证据**（临时探针，`cargo test -p crabmate probe_`，跑完即删）：

| 输入 | 结果 |
|---|---|
| `modify_file` + **`cargo_test` 专用工具**跑通 100 测试 + 自述完成 | **`NotApplicable`** |
| `modify_file` + **`run_command` 跑 `cargo test`** + 自述完成 | **`Satisfied`** |
| 仅 `modify_file` + 自述完成（任务「把常量改成 42」） | **`Satisfied`** |

第二、三行合起来说明：**同一语义事实，仅因工具名不同，判定结果相反**；而**零验证的纯写任务却判完成**。

四个结构性缺陷：

**① 专用工具被排除。** `generic_tool_is_build_or_test_success` 前置 `tool_name == tool_names::RUN_COMMAND`。
用规范的 `cargo_test` / `cargo_check` / `cargo_clippy` 反而判不出——**越规范用法越失效**。

**② 纯写任务无验证即判完成。** `generic_impl_intent_satisfied` 对 `write_change` 只要求 `write_success`
（工具名命中 `modify_file` 等）+ 自述「完成」，不要求任何验证性证据 → 假阳性。

**③ 判定混入 assistant 自述关键词。** `completion` 取自 `assistant_completion_claim`（正文含「完成/成功/done」），
且与工具证据是 **AND** 关系。这使「自述」成为必要条件之一，也带来反向误判（`has_hard_blocker` 含「失败」，
任务「修复登录失败」成功后复述原文即判不完成）。

**④ 检测器只有 1 个且写死 C++。** `GOAL_EVIDENCE_DETECTORS` 是单元素数组，只因 `is_program_build_run_request`
不命中才落到上面的通用兜底；两条路**都是文本启发式**。

**最致命的是范式问题，不是覆盖不足**：综合任务（如「重构 X 并保证行为不变」）没有单一 `exit_code` 能代表完成，
而当前实现试图用「输出了哪些词」推断「语义是否达成」。继续补关键词只会移动误判面。

后果：早停门既**判不出真完成**（③④ + ①），也**可能误判完成**（②）。两个方向都有错，且都不可观测
（`NotApplicable` 不带原因）。

### 3.3 B3 探索与改动挤在同一消息窗

读文件、跑测试、失败重试全部灌进父 `messages`，靠事后压缩缓解。子 Agent 委托恰好解决这一点
（独立 messages + 只回传 `summary` + `key_paths`），且设计已定稿、前置重构已完成。

### 3.4 B4 审批缺「按任务预声明」，而非缺持久化

审批与澄清问卷都是回合内**同步阻塞**。需澄清一处易夸大：`AllowAlways` **已会持久化**
（`tool_host/tool_approval` → `persist_allowlist_key`），键还带等级作用域（`l3|git`）。所以「同一个键批一次
长期有效」**已经成立**，不是「只有逐次批准」。

真实缺口更窄：缺的是**「按任务一次声明一组键」的打包能力**——用户无法在开工前说「本任务大概会用到
git / cargo / gh 这几类，别再逐条问我」，只能被动等模型撞上一次、批一次。这在长任务里表现为**打断次数多**
而非**权限面不足**。

---

## 4. 增强方案（按依赖顺序，每层可独立交付）

### L1（前置）完成判定层重建：证据采集 + LLM 语义判定

**定位**：本文的 **P0 前置项**，不再是「低成本顺手做」。它卡住 L2（`TaskState` 需要可信子项完成信号）、
L6（读三态），且当前实现双向误判（§3.2 实测）。**判定层不可信，其上的续航与预算分配都建在流沙上。**

**为什么不是「补关键词」**：综合任务（「重构 X 并保证行为不变」）没有单一 `exit_code` 能代表完成。
用「输出里有哪些词」推断「语义是否达成」，继续加词只会移动误判面。**问题在范式，不在覆盖。**

#### 两层职责划分

| 层 | 回答的问题 | 手段 | 特性 |
|---|---|---|---|
| **① 证据采集** | **事实是什么** | 结构化：退出码、文件改动、测试通过/失败数、diff 摘要 | 确定、零 LLM、可金样 |
| **② 完成判定** | **这些事实够不够算完成** | **LLM 语义判定** | 灵活、需约束 |

关键纠正（相对本文早期草稿）：**LLM 是第 ② 层主体，不是「结构化判不出时的补充」**。
结构化判定只覆盖单次调用可自证的情形，对综合任务不成立。

#### ① 证据采集层

把多次工具调用归一为结构化事实集合，每条带稳定 id（`ev_1`、`ev_2`…）：

- 退出码（含 `cargo_test` / `cargo_check` / `cargo_clippy` 等**专用工具**，修复 §3.2 缺陷 ①）
- 文件写入/修改（路径 + 工具名）
- 测试通过/失败数（从 `structured_payload` 或 stdout 解析）
- 关键输出片段（截断）

**这一层不清言完成**，只陈述事实上发生了什么。必须零 LLM，保证可回归。

#### ② LLM 语义判定层

输入 = **用户任务 + ① 的结构化事实集合**（不含原始消息堆，尤其**不含 assistant 自述正文**）。

输出必须带证据锚点：

```json
{ "status": "satisfied" | "partial" | "unsatisfied",
  "evidence_refs": ["ev_3", "ev_7"],
  "missing": ["未验证边界条件"] }
```

**核心约束：`satisfied` 必须配非空 `evidence_refs` 且引用真实存在的 id；否则降级为 `unsatisfied`。**

这条专门针对 §3.2 缺陷 ② ③：LLM 可以说「完成」，但必须指向具体事实，不能凭 assistant 自己的措辞下结论。
防的不是「LLM 判断」，而是**无锚点的判断**。

其余约束：

- **fail-open**：LLM 失败/超时/JSON 解析不了 → 退回 `NotApplicable`（维持现状，绝不因此早停）
- **单向**：只能把 `NotApplicable` 提升，不能否决 ① 已给出的确定性结论
- **有界**：`max_tokens ≤ 512`，计入 `TurnBudgetCounter`，默认关闭
- **复用既有范式**：照 `src/agent/per_plan_semantic_check.rs` 的 JSON-only + 短 prompt + fail-open 写法，
  复用 `final_plan_semantic_check_*` 风格的开关，不新造一套配置面

#### 调用点（成本关键）

不是每批工具后调用，而是**候选终止点**，一回合各一次：

1. 模型给出终答（无 `tool_calls`）时 —— `per_reflect_after_assistant` 所在的同一时刻
2. 达到预算阈值时 —— 决定「带半成品停」还是「再给一轮」

`docs/design/audience_critic_role.md` 的锚点 C/D 就在这条线上，应当是**同一位置的消费者**，
不另开并行阶段机（与 `run_loop_state_ownership.md` 一致）。

#### 综合任务靠分解降维

综合任务难判，是因为它是**一个大的语义判断**。而规划层已在分解（`AgentReplyPlanV1.steps`）：

```
综合任务完成 = 各子项完成 ∧ 聚合规则满足
```

每个**子项**粒度小得多，往往能落到 ① 的结构化证据；整体聚合是确定性的。
`TaskState` 存的正是「子项 → evidence_refs」映射——**L2 不只是续航，它同时是把综合任务变成可判定问题的手段**。

#### 废弃项

关键词路径（`assistant_completion_claim` / `generic_intent_satisfied` / `GOAL_EVIDENCE_DETECTORS`
及其 C++ 检测器）在 ①② 落地并覆盖后应**整体移除**，不要保留为 fallback——否则会与新层并存，
重演「两套实现语义不对齐」的老问题（§3.2 现状的成因）。

**✅ 已于 #1415 完成**：上述三处全部删除，`policy/task_level_evidence/` 只剩 `evidence.rs`
（结构化事实采集）。**代价**：早停现在完全依赖 LLM 可用性——关掉 `completion_judge_enabled`
或缺 `API_KEY` 时任何任务都不早停，回合会跑到迭代上限。这一取舍是有意接受的。

#### 落点

`policy/task_level_evidence/`（重写为证据采集 + 判定两层）、`cm_agent/acceptance/`（复用 `AcceptanceSpec`
做 ① 的字段归一）、`policy/turn_completion_decision.rs`（消费新三态；`build_or_test_intent` 冲突门
已于 #1415 删除）、新增一个 `*_semantic_check` 风格的侧向调用模块。

#### 风险与缓解

- **假阳性（判完成但没完成）代价最高**：提前终止交半成品。缓解＝上面「`evidence_refs` 非空」硬约束 +
  fail-open 只退不放 + 金样覆盖三类任务（纯写、构建测试、纯分析）。
- **LLM 顺从偏差**：输入剔除 assistant 自述，切断自评回路；可选要求判定模型 ≠ 执行模型（见 §8）。
- **成本**：锚定候选终止点 + `max_tokens ≤ 512` + 默认关闭。

#### 验收

- §3.2 三个实测探针全部按预期改变：`cargo_test` 专用工具 → 不再 `NotApplicable`；
  纯写无验证 → 不再 `Satisfied`。
- `fixtures/turn_completion_golden.jsonl` 的 `suppress_replanning` / `redundant_tools` 两档各有
  `confirmed: true → allow` 与 `confirmed: false → deny` 用例（`early_stop` 档随第 ① 层一并删除，#1415）。
- 综合任务场景：分解释义下子项判定可独立回归。
- 关闭该功能时行为逐字不变。

### L2 跨回合任务状态与续跑（自主性核心）

**做法**

- 新增 `TaskState`：目标、子项列表及逐项状态、已完成证据摘要、剩余预算、暂停原因。
  持久化到 `.crabmate/tasks/<id>.json`，以 `conversation_id` 关联。
- 回合因预算耗尽 / 待用户输入结束时，写 `status = paused` + `reason`，**不**丢弃进度。
- 由 `chat_job_queue` 或复用 `scheduled_agent_task` 的调度按其触发下一回合，注入精简 `TaskState`
  而非全文历史。
- 硬闸：`max_autonomous_turns`、累计 Token 上限、`stop_on_approval_required`、用户可随时中断。
  **默认关闭**。

**落点**：`src/cm_config` 新增 `[agent.autonomy]` 节（含 `CM_*` 覆盖与热重载语义，须同步
`docs/配置说明.md`）；注入点 `src/agent/model_context_view.rs`；触发点 `src/chat_job_queue/`。

**风险与缓解**（必须写进配置说明，不得淡化）

- 自主循环的成本与不可逆操作面。缓解＝把现有三重预算从「单轮」改为**累计**计费，
  且 `permission_level` 上限在启动期冻结的行为不变（`POST /config/reload` 仍只能降）。
- 与 SSE 契约的关系：续跑发生在**新回合**，不改变单回合内的终态序（`docs/SSE协议.md` 不变）。

**验收**：同一「修好 CI 并跑通全量测试」任务，人工介入次数由 N 降到 1；关闭该功能时对照行为逐字不变。

### L3 `delegate_task` P1（设计已定稿，直接执行）

**做法**：按 `docs/design/subagent_delegation.md` §5 / §6 接线 4 处
（`RunAgentTurnAttach` 加能力对象、`DispatchToolParams` 加分组、`HandlerId::Delegate` 注册、
`tool_execution_host` 与 `HandlerId::Workflow` 并列特判）。`AGENT_LAYER_HANDLERS` 已是可枚举清单，加一行即可。

**前置条件**：先落地 `multi_agent_collaboration.md` §6.6 的**委托粒度门限**（≥3 次工具调用 /
需上下文隔离 / 可并行）。单次读文件、单次检索**禁止**委托，否则只会放大成本。

**约束**

- `CrabmateParallelToolDispatch` 必须拒绝委托：只读并行批的前提是「无副作用」，跑子回合会打破它。
- 子回合预算不含工具调用次数（`TurnBudgetCounter` 仅有 LLM 次数 / Token / 墙钟），首版以墙钟兜底。

### L4 审批从「撞上一次批一次」到「按任务预声明一组」

**前提澄清**：持久化已就绪（`persist_allowlist_key` + 等级作用域），所以 L4 **不是**补持久化。

**做法**：引入回合级 `autonomy_profile`，其内容 = 现有 `permission_level` 上限 + **可枚举的**审批白名单键
（沿用既有 `l3|git` 键格式）+ 预算，组合为可审计策略包。用户在开工前一次声明「本任务会用到这几类」，
回合内不再逐条打断。

**为什么不是新增权限档**：权限裁决链必须唯一。策略包只做「本轮预先声明会用到哪些既有键」，
不改变任何单次裁决的结果；越出策略包范围仍走交互审批。

**边界**：L4 减少的是**打断次数**，不是**权限面**；不得以「反正要预声明」为由放宽 `permission_level` 收窄。

### L5 记忆从会话孤岛到项目级

按 `docs/design/agent_self_evolution_roadmap.md` 推进 P1（A 层分层 scope）→ P2（B 层效果反馈）→
P3（C 层护栏），**不要跳过 A 直接扩 E 层沉淀面**——否则噪音先放大、无从回收。

本文只引用，不重复其字段设计。

### L6 证据密度驱动的预算再分配

现有 `budget_degradation_enabled`（默认关）已是降级通道。在此之上，允许把已 `Satisfied` 子项的剩余预算
让给 `Unsatisfied` 子项。**依赖 L1 的三态信号**，故排在最后。

---

## 5. 实施顺序（待评审确认）

| 阶段 | 内容 | 依赖 | 说明 |
|---|---|---|---|
| S0 | 本文评审定稿 | — | 只出方案，不写码 |
| **S0.5** | **L1 完成判定层重建（P0 前置）** | — | **所有后续阶段的公共前置**。判定层不可信，`TaskState` 与预算再分配都无从验收 |
| S1 | 步后抑制接线（`evaluate_turn_suppress_replanning`） | S0.5 | 小改动，依赖可信三态 |
| S2 | L2 `TaskState` 与续跑 | S0.5、S1 | 自主性核心；默认关闭，先做单实例内 |
| S3 | L3 `delegate_task` P1 | 独立 | 与 S2 无编译冲突，可并行 |
| S4 | L4 `autonomy_profile` | S2 | 有了续航才需要减少打断频率 |
| S5 | L5 记忆 A→B→C | 独立 | 与 S2/S3 不同模块，可并行 |
| S6 | L6 预算再分配 | S0.5、S2 | 依赖三态信号与累计预算 |

**为什么 S0.5 前置**：L2 的 `TaskState` 存的是「子项 → 完成状态」，L6 读的是三态信号。
判定层判不出（实测）或误判（实测），这两层的正确性都无从谈起。先做 L2 等于在流沙上盖楼。

**不建议的顺序**：先做 L3 再补 L2。没有跨回合状态时，再多的子 Agent 也只是一次性探索，
无法把结果累积成「完成」。

---

## 6. 验证方法

「自主性变强」必须可量化，否则无法判断增强是否有效。

1. **回归对照**：用既有 benchmark 流程（`--benchmark` + `--batch`）跑「关闭 L2」vs「开启 L2」两组，
   比较任务成功率与人工介入次数。
2. **效率指标**：单位任务的多余探针回合数、重复工具调用率、上下文压缩触发次数。
3. **防退化门禁**：L5 上线后回归对照不应下降；下降即视为护栏失效（与自进化路线图 §6 一致）。
4. **契约不变性**：`docs/SSE协议.md` 的终态序、`fixtures/` 各金样在默认配置下逐字不变。

---

## 7. 明确不做（拒绝项）

| 拒绝 | 原因 |
|---|---|
| 恢复全局单表 FSM / `hierarchy` / `staged` | 已归档并移除，`phase_vocabulary` 明写「三机并存、勿合并」 |
| 恢复 L2 意图分类 | 已退役；配置 `deny_unknown_fields` 会直接拒绝 |
| 把 `api_key` 塞进 `DispatchToolParams` | 破坏「工具执行层不持凭据」分层，委托设计 §3 已否决 |
| 新增 `permission_level` 档位表达自主性 | 与六轴投影、等级作用域审批键冲突，见 §1.2 |
| 用「放宽预算上限」替代续航 | 只是把中止点推后，未解决进度丢失；且放大成本与不可逆面 |

---

## 8. 开放决策（需评审拍板）

1. **L2 的触发者**：由 `chat_job_queue` 内部自驱，还是复用 `scheduled_agent_task` 的 cron 面？
   前者语义更直接，后者可复用既有审计与开关。
2. **L2 的暂停边界**：遇到审批请求是「暂停等用户」还是「跳过该项继续其余子项」？前者更安全，后者更自主。
3. **`TaskState` 的可见性**：仅服务端内部，还是经 HTTP 暴露给 Client 展示（需同步 `cm_api_contract`
   与 `docs/命令行与路由.md`）？
4. **L1 判定模型的选型**：复用 `planner_model`（`per_plan_semantic_check` 的既有做法），还是要求
   「判定模型 ≠ 执行模型」？后者对自评偏差更有效，但多一层部署约束。
5. **L1 `partial` 的处置**：判出「部分完成」时，是继续跑（消耗预算换完整度），还是停下把 `missing`
   交给用户？这直接决定 L2 续跑的语义。
6. **L1 `evidence_refs` 的严格度**：只校验「id 存在」，还是进一步校验「该证据语义上支持结论」？
   前者可机器校验，后者需二次 LLM，成本翻倍。

---

## 9. 跟踪

- 主 Issue：[#1386](https://github.com/noisystreet/CrabMate/issues/1386)（路线级共识与阶段进度）。
- 前置项 S0.5：[#1387](https://github.com/noisystreet/CrabMate/issues/1387)（完成判定层重建）。
- 每个阶段（S1–S6）在开工前按 `AGENTS.md` §5 单独建 Issue，PR 正文用 `Closes #` / `Refs #` 关联。
- 某阶段落地后：从 `docs/待办清单.md` 删除对应条目，并把本文该阶段状态改为「已落地（PR #）」。

## 10. 修订记录

| 日期 | 变更 |
|---|---|
| 初稿 | 提出 B1–B4 与 L1–L6 |
| 评审修正一 | **更正 §3.2**：原判「主路径完成判定靠模型自述」**错误**。实际 `turn_early_stop_allowed` 早已接线并做证据核对。据此**重写 L1**、**收窄 §3.4/L4**（`AllowAlways` 已持久化）、调整 §5/§8。 |
| 评审修正二（实测驱动） | 用临时探针实测（跑完即删，源码未改）确认三个事实：① 专用 `cargo_test` 被判 `NotApplicable`，而等价的 `run_command` 判 `Satisfied`（**同事实因工具名不同而结论相反**）；② 纯写任务「改文件 + 自述完成」**无验证即判 `Satisfied`**（假阳性）；③ 判定混入 assistant 自述关键词（AND 关系）。据此把 L1 **改为「判定层重建」两层架构**（结构化证据采集 + LLM 语义判定，`evidence_refs` 强制锚点），并把 L1 提升为 **S0.5 前置项**。 |
