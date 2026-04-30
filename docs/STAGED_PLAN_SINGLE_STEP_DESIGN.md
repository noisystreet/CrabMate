# 分阶段规划：单步规划（Planner 每轮仅 1 条 `steps`）设计

**状态**：主要已实现（持续完善中）  
**目标读者**：维护者；实现前须与 **`docs/DEVELOPMENT.md`**（分阶段规划）、**`docs/CONFIGURATION.md`**、**`docs/SSE_PROTOCOL.md`** 及 **`crates/crabmate-sse-protocol`** 变更策略对齐。

---

## 实现进度（截至当前）

### 已实现

- 固定单步约束（无新增配置开关）：在 `agent_reply_plan` v1 校验中，`no_task=false` 时默认仅允许 1 条 `steps`。
- 违规处理采用“硬拒绝”：多步计划触发 `TooManySteps` 校验错误（进入既有无效规划处理链路）。
- staged 执行流已改为“步后再规划”：每完成一步后自动重入下一轮无工具规划，直到 `no_task` 或终止条件。
- 增加循环保护：单用户回合内分阶段单步规划轮次上限（防止异常无限循环）。
- `workflow_validate_only` 绑定优先例外：当多步且每步都带 `workflow_node_id` 时放行，后续由绑定校验规则兜底。
- 绑定优先例外已收紧为“**仅在存在 validate-only 绑定上下文时**放行”：在 staged 的主规划、ensemble 次规划与 patch 规划解析路径统一生效，避免非绑定场景误放行多步。
- 补充/调整单测：覆盖“多步拒绝”与“workflow 绑定多步例外”关键行为。

### 近期已补充

- 文档语气已从“待设计”收敛为“实现说明 + 风险/余项”。
- 已与 `docs/CONFIGURATION.md` / `docs/DEVELOPMENT.md` 对齐“固定单步、无新增配置项”的决策。
- 已补充回归测试，锁定“单步执行后重入下一轮规划并收敛退出”与“轮次上限防护”行为。
- 已增加“意图门控后再入 staged”策略：仅识别为执行类（`IntentAction::Execute`）的回合进入分阶段规划；普通问答默认走常规外环，避免一问多答与无效规划轮。

---

## 0. 与产品目标的对齐：「单智能体 + 工具循环」

仓库内**「单智能体 + 工具循环」**的既有实现，主要指 **`planner_executor_mode = single_agent`** 且 **`staged_plan_execution = false`** 时的 **`run_agent_outer_loop`**：同一对话链路、由模型在循环中反复选择工具直至终答。

**本设计所服务的演进目标**是：在仍使用 **分阶段规划**（`staged_plan_execution = true`）与 **`agent_reply_plan` v1** 的前提下，让**运行时形态**更接近上述模式——

- **单智能体**：不引入 hierarchical 的 Manager/Operator 多角色编排；主对话仍为**单一助手主体**与既有 P/R/E 外环（与现 `staged` 路径一致）。
- **工具循环**：每一步子目标内的执行仍是 **工具调用循环**；规划轮退化为**高频、短视界**的「下一步意图/约束」注入，而不是一次吞吐很长 `steps[]` 的静态队列。

因此：**「Planner 每轮仅 1 条 `steps`」是实现手段**；**「逼近单智能体 + 工具循环」是行为层面的目标表述**。下文技术条款均从属于该对齐关系。

---

## 1. 背景与动机

- **现状**：`staged_plan_execution = true` 时，无工具规划轮可产出 **`agent_reply_plan` v1**，`steps` 常为**多条**；执行器按 `steps` 顺序逐次进入外层循环（每步仍含 P/R/E 与工具调用）。
- **目标**：在**保留**「规划轮 JSON + 分步执行 + SSE 时间线」的前提下，约束 **Planner 每轮输出的 `steps` 长度上限为 1**（`no_task` 时仍为空），使整体更接近 **滚动视界（rolling horizon）**：每完成一步（或等价语义），在同一用户任务下**再次**进入无工具规划轮，产出**下一步**——从而在节奏上贴近 **「观察环境 → 再决定下一步」** 的 ReAct 式单智能体循环，而非「首轮即排定长队列再依次清空」。
- **非目标**：不替代 **`planner_executor_mode = hierarchical`**。  
  **`staged_plan_execution = false`** 下的 **`run_agent_outer_loop`** **已是**单智能体 + 工具循环；本设计**不是**重复实现该路径，而是让 **staged 路径在启用时**通过单步 + 步后 replan **向该形态收敛**（仍保留结构化规划与相关 SSE/审计能力）。

---

## 2. 行为定义

### 2.1 解析后不变量

在**启用本特性**且本轮规划**非** `no_task` 时，对解析得到的 **`AgentReplyPlanV1`** 须满足：

- **`steps.len() == 1`**。

当 **`no_task == true`** 时，保持现有契约：**`steps` 必须为空**（见 `plan_artifact::validate_agent_reply_plan_v1`）。

### 2.2 违反时的策略（须二选一并在实现中统一）

| 策略 | 行为 | 优点 | 缺点 |
|------|------|------|------|
| **A. 硬拒绝** | 与 `EmptySteps` / 非法 `id` 类似，返回明确 **`PlanArtifactError` 变体**（如 `TooManySteps { max: 1, got: n }`），触发既有 **plan 重写 / 用户可见错误** 路径 | 语义清晰、易测 | 模型常输出多步时重写次数与费用上升 |
| **B. 静默截断** | 只取 `steps[0]`，丢弃后续（可打 `warn` 日志） | 鲁棒 | 与「Planner 意图」不一致，审计困难；**默认不推荐** |

**建议**：默认 **A**；若日后需要 B，须单独配置键并文档标明风险。

### 2.3 与「多轮规划」的关系

- **单步约束只作用于「每一轮无工具规划轮」产出的 JSON**，不自动增加「每用户回合必须规划几次」；**下一轮规划**仍由现有外环（执行完当前 `steps` 后是否再进入规划轮）决定。
- 若当前实现是「一次规划、队列执行完所有 `steps` 再结束用户回合」，则启用单步后变为：**每段执行只消费 1 步 → 需在同一用户任务下再次触发规划轮**才能继续。此处**必须与 `agent_turn::staged` 实际控制流逐字核对**（实现阶段任务）：若今日代码仅在首轮 P 一次，则须扩展为 **步后 replan** 或 **将「用户回合」拆成多段 planner 调用**；本设计稿假定产品接受 **「步后再次无工具 P」** 的 API/会话成本。

### 2.4 进入门控（意图识别）

- 分阶段规划不再对所有回合无条件生效；在 `run_agent_turn_common` 处增加回合级门控。
- 门控复用现有 `intent_pipeline`（L0/L1；与阈值配置一致），仅当决策动作为 **`IntentAction::Execute`** 时，允许进入 `staged` / `logical_dual_agent` 路径。
- 对于 `qa.meta`、`qa.explain`、寒暄、澄清未确认等非执行意图，直接走 `run_agent_outer_loop`，避免“先规划后降级”导致的双回答。
- 该门控是执行路径选择，不改变 `agent_reply_plan` v1 的单步校验契约。

---

## 3. 配置面结论（已定稿）

- 当前实现已采用**固定单步**策略：`no_task = false` 时默认仅允许 1 条 `steps`。
- **不新增** `staged_plan_max_planner_steps`（或等价）配置项，也**不新增**对应环境变量。
- 例外仅保留给 `workflow_validate_only` 绑定场景（多步且每步显式 `workflow_node_id`），由后续绑定校验兜底。
- 因为无新增配置键，本特性不涉及 `POST /config/reload` / `GET /status` 的新增暴露项。

---

## 4. 与现有子系统的交互（实现前核对清单）

### 4.1 `plan_artifact` 校验

- 在 **`validate_agent_reply_plan_v1`** 之后或之内增加对 `max_steps` 的检查；错误类型进入既有 **`PlanArtifactError`** 映射，保证 **plan_rewrite**、SSE **`reason_code`** 与前端分支可消费（若新增 `reason_code`，须走 **`api-sse-chat-protocol.mdc`** 清单：`sse_dispatch`、**`control_classify`**、**`fixtures/sse_control_golden.jsonl`**、`cargo test golden_sse_control`）。
- 调用约定：涉及 validate-only 绑定语义的路径，统一使用带上下文参数的解析接口（`parse_agent_reply_plan_v1_with_validate_only_binding_ids` / `parse_agent_reply_plan_v1_from_assistant_message_with_validate_only_binding_ids`），避免无上下文解析误放行多步绑定例外。

### 4.2 工作流 `workflow_validate_only` 节点绑定

现有 **`validate_plan_binds_workflow_validate_nodes`** 要求 **`steps.len() == nodes.len()`** 且每步带 `workflow_node_id`。当 **`nodes.len() > 1`** 时，与 **`steps.len() == 1`** **互斥**。

**须定义优先级**（建议其一写死进实现与文档）：

1. **绑定优先（已实现且带上下文门控）**：当本回合适用 `validate_plan_binds_workflow_validate_nodes` 且 `nodes.len() > 1` 时，**跳过**单步上限（允许本轮 `steps.len() == nodes.len()`）；但若不存在 validate-only 绑定上下文，则不放行该多步例外，或  
2. **单步优先**：启用单步时，若绑定规则要求多步，则 **报错** 并提示用户关闭单步或拆分工作流回合。

推荐 **1（绑定优先）**，否则 validate-only → Do 路径无法在一次规划内满足。

### 4.3 `final_plan_require_strict_workflow_node_coverage` 等

- **`validate_plan_covers_all_workflow_node_ids`** 等与 `steps` 数量相关的规则：在「绑定豁免」或「多步例外」分支中写清，避免组合爆炸无文档。

### 4.4 规划优化轮（optimizer / ensemble）

- 现有逻辑在 **`steps.len() >= 2`** 时才进入部分优化路径（见 `DEVELOPMENT.md` 描述）。**`steps.len() == 1`** 时自然跳过；**无行为变更需求**，但须在 UI/日志中避免「误报优化未运行」为故障。

### 4.5 规划提示词（`PLAN_V1_SCHEMA_RULES` / `staged_plan_phase_instruction`）

- Schema 规则文本须明确：**在单步模式下**「除 `no_task` 外 **`steps` 仅含一项**」。
- 若用户自定义 **`staged_plan_phase_instruction`**，文档说明 **不得与单步约束矛盾**（或实现侧在加载时给出 `config` 自检警告）。

### 4.6 前端与 CLI

- **`staged_plan_todo`**、**`agentPlanDisplay`**：长期仅 1 步时，待办 UI 可能退化为「当前一步」；需产品接受或增加「已执行步数 / 未知总数」的展示策略（不要求在本设计稿定稿视觉稿，实现 PR 中处理）。
- **CLI** `format_plan_steps_markdown_for_staged_queue`：单步时队列摘要仍合法。

### 4.7 `logical_dual_agent`

- 若与 **`single_agent` + staged** 共用同一套 `agent_reply_plan` 解析与校验，**单步约束应对两者同时生效**（除非另有产品决策）。本设计稿默认 **一致生效**。

### 4.8 三端（CLI / TUI / Web）

- 遵守 **`.cursor/rules/cli-tui-web-shared-logic.mdc`**：配置键与行为在 **Web + CLI + TUI** 一致；仅 Web 专用 UI 除外。

---

## 5. 测试与回归

- **`plan_artifact` 单元测试**：`steps` 为 0/1/2 在开关 on/off 下的通过/失败矩阵；`no_task` 边界。
- **与工作流绑定组合**：`nodes.len()==1` 且单步；`nodes.len()>1` 与豁免策略。
- **意图门控回归**：覆盖“普通问答（如 `你有哪些技能`）不进入 staged”与“执行请求进入 staged”（`agent_turn::tests::staged_intent_gate_tests`）。
- **SSE / golden**：若新增控制面或 `reason_code`，按仓库规则更新 **`fixtures/sse_control_golden.jsonl`** 等。
- **前端**：`cargo check --target wasm32-unknown-unknown`；大改时 `trunk build`。

---

## 6. 文档与发布

- **`docs/CONFIGURATION.md`**：新键、默认值、环境变量、与 `workflow_validate_only` 的优先级。
- **`docs/DEVELOPMENT.md`**：在「分阶段规划」小节增加指向本文的链接及一句行为摘要（实现合并时做）。
- **`README.md`**：若对用户可见「推荐配置场景」，可加一句（可选）。
- **`docs/BENCHMARK_PLANNING.md`**：无需引用；本特性与 benchmark 无强耦合。

---

## 7. 风险与开放问题

- **API 成本**：步后 replan 若增加无工具轮次数，须可观测（日志 / trace），并考虑与 **`plan_rewrite_max_attempts`** 的叠加心理预期。
- **模型遵从度**：仅靠 schema 文字可能仍产出多步；**A 策略 + 重写** 为默认缓解；可考虑在重写 user 中附带「上轮多步已拒」的短因。
- **会话持久化**：多段 planner assistant 消息增长与 **`max_message_history`** 截断策略是否削弱「前文对下一步的规划」；若有问题，后续独立议题（本设计不展开）。

---

## 8. 参考代码锚点（实现阶段使用）

| 区域 | 路径 |
|------|------|
| 分阶段入口 | `src/agent/agent_turn/staged/mod.rs`（`run_staged_plan_then_execute_steps` 等） |
| 规划 JSON 类型与校验 | `src/agent/plan_artifact.rs`（`AgentReplyPlanV1`、`validate_agent_reply_plan_v1`、workflow 绑定） |
| 顶层分支 | `src/agent/agent_turn/mod.rs`（`run_agent_turn_common`） |
| 配置类型 | `src/config/types.rs`、`finalize.rs`、`env_overrides.rs`、`hot_reload.rs` |

---

**文档维护**：本页随实现 PR 更新「状态」与「开放问题」闭合情况；与 `agent_reply_plan` v1 契约变更同步修订。
