# PER 编排：用显式状态机收拢分支（设计）

**状态**：设计稿（**未**承诺实现时间表与范围）。**受众**：维护 `agent_turn`、`per_coord`、分阶段规划与分层 Agent 的开发者。  
**语言**：中文。  
**关联文档**：

- **`docs/开发文档.md`**（**`run_agent_turn_common`**、P/R/E、`planner_executor_mode`、**`staged_plan_*` / `final_plan_*`**）
- **`docs/规划执行验证架构.md`**（结构化 P-E-V 与 `plan_rewrite` 正交关系）
- **`docs/HIERARCHICAL_MULTI_CM_ARCHITECTURE.md`**（分层模式与 Manager 反思）
- **`docs/design/agent_state_management.md`**（更广义的会话/产物状态，与本设计正交）
- 源码：`src/agent/agent_turn/mod.rs`（P/E/R 定义）、`src/agent/per_coord/`（`mod.rs`、`final_plan_gate.rs`）、`src/agent/agent_turn/staged/mod.rs`、`src/agent/agent_turn/staged/orchestrator.rs`、`src/agent/workflow_reflection_controller.rs`

---

## 1. 背景与问题

### 1.1 当前 PER 在代码中的形态

- **P（Plan）**：一次 `llm::complete_chat_retrying` 调用，产出 assistant 或 `tool_calls`（见 `agent_turn/mod.rs` 注释）。
- **E（Execute）**：`execute_tools` / 工作流执行路径。
- **R（Reflect）**：`per_reflect_after_assistant`；终答无 `tool_calls` 时由 **`PerCoordinator::after_final_assistant`** 决定是否结束、要求 **`agent_reply_plan` v1** 重写，或进入侧向 **`per_plan_semantic_check`**。

此外，**分阶段规划**（`staged/mod.rs`）在单轮内叠加：无工具规划轮 → 解析 `agent_reply_plan` → 按步多次进入 **`run_agent_outer_loop`**；其间还有优化轮、集成、patch planner、`no_task` 降级等分支。

**分层模式**（`hierarchy`）中，Manager 的 **`reflect_and_replan`** 对验证失败做 JSON 反思，与上路径**共享心智模型**但**不同代码路径**。

### 1.2 「分支分散」具体指什么

| 表现 | 位置 / 说明 |
|------|-------------|
| 终答是否必须含规划 | `FinalPlanRequirementMode` + `PlanRequirementSource` + `require_plan` 多段推导（`per_coord/final_plan_gate.rs`） |
| 规划静态校验与重写 | `after_final_assistant` 内长链：解析、层数、workflow 节点子集/全覆盖、validate-only 绑定、重写次数、语义检查挂起等 |
| 分阶段主循环 | `staged/mod.rs`：`for` 步、patch 重入、优化/集成/两阶段 NL 等交叉 `if` |
| 终答门控入口 | `final_plan_gate::after_final_assistant`：**始终**经 **`run_final_plan_gate(phase, …)`**；`NoRequirement` 时不扫描 `workflow_validate` 缓存（避免无需求路径的副作用） |
| 顶层回合形态 | `run_agent_turn_common` / `run_dispatch`：`tracing` 字段 **`turn_orchestration_mode`**（**`turn_orchestration::TurnOrchestrationMode`**），与分层 / 非分层主路径对齐 |
| 分层内子阶段（观测） | `agent_turn/hierarchy.rs`：`tracing` 字段 **`hierarchical_phase`**（`intent_gate` / `discourse_fallback_outer` / `router_manager_runner` / …），与顶层 **`turn_orchestration_mode=hierarchical`** 正交 |
| 可观测子阶段 | 已有 **`AgentTurnSubPhase`**（`planner` / `executor` / `reflect`）与 SSE **`sub_phase`**，与**内部决策状态**未一一对应 |

问题不是「缺少功能」，而是：**合法转移路径**分散在多个布尔/计数组合里，新加一条规则时容易漏改或产生不可达组合。

### 1.3 设计目标

1. **单处权威**：对「本步结束后下一步做什么」的决策，优先通过 **(状态, 事件) → (下一状态, 效果)** 表达。  
2. **类型收紧**：用枚举缩小合法组合；非法组合在 `match` 中显式处理或拒绝。  
3. **与现网行为可渐进对齐**：分阶段把 **`after_final_assistant` 子逻辑** 或 **分阶段单步** 收拢，避免大爆炸式重写。  
4. **不改变**以下**已存在契约** unless 另开版本化工作：`AfterFinalAssistant`、`RunAgentTurnError` / **`sub_phase`**、SSE 控制面、**`plan_artifact` v1** 字段语义。

---

## 2. 设计原则

| 原则 | 说明 |
|------|------|
| **状态要少** | 状态机只表达**编排**；`messages` 长度、工具摘要等放入只读 **Context**，避免状态爆炸。 |
| **事件要显式** | 「收到终答」「解析失败」「重写次数 +1」「语义 LLM 完成」等应是具名事件，不是隐式在函数尾部继续跑。 |
| **效果与 IO 分离** | 转移函数尽量产出 **数据效果**（追加哪条 `Message`、要发的 SSE 种类）；真实 `complete_chat_retrying` / 写 `messages` 保留在 `agent_turn`  driver。 |
| **与 `plan_rewrite` 正交** | 形式与绑定类失败继续走现有 **`plan_rewrite` / `PlanRewriteExhaustedReason`** 语义，不另造一套码（见 `规划执行验证架构.md`）。 |
| **可测** | 纯表驱动或纯函数可单测；需要 `messages` 的用 fixture 向量。 |

---

## 3. 建议拆分：两台「机」

全局**不宜**用单一 FSM 覆盖 `run_agent_turn` 全部分支（Hierarchical / staged / single 差异太大）。建议 **两台独立 FSM**，共享词汇但不同模块实现。

### 3.1 终答规划门控 FSM（Final Plan Gate）

**范围**：仅替代或包裹 **`PerCoordinator::after_final_assistant`** 内的决策树，不替代整个 `PerCoordinator`（工作流反思注入、**`PlanRequirementSource`** 置位仍由既有 API 完成）。

**状态（示例，可迭代）**：

- `NoRequirement`：当前配置下不要求终答规划。  
- `CheckStructuredPlan`：需要可解析的 `agent_reply_plan` 及与 workflow/validate-only 等静态规则一致。  
- `PendingSemanticLlm`：静态已通过，挂起侧向 **`per_plan_semantic_check`**（与现有 `StopTurnPendingPlanConsistencyLlm` 对齐）。  
- `Exhausted`：已达 `plan_rewrite_max_attempts` 等（与 `StopTurnPlanRewriteExhausted` 对齐）。

**事件（示例）**：

- `FinalAssistantArrived`（已推入 `messages` 的 assistant 引用/预览）  
- `SemanticLlmCompleted` / `SemanticLlmFailed`  
- `PolicyOrSourceChanged`（如热重载后需重置门控，若采用）

**输出**：保持现有 **`AfterFinalAssistant`**，避免上层大改；FSM 实现为对 `msg` + `messages` + `ctx` 的一次 `step()` 或 `reduce()`。

### 3.2 分阶段回合 FSM（Staged Turn Orchestrator）

**范围**：`run_staged_plan_then_execute_steps` 及其子路径中的 **回合级** 结构（不是单步内 `outer_loop` 的每一圈 P→E）。

**状态（示例，可迭代）**：

- `PrePlan`：准备首轮/补丁轮无工具规划（含是否跑 ensemble、优化、两阶段 NL 等**策略位**可挂在子状态或 Context）。  
- `PlanReady`：已得合法 `AgentReplyPlanV1`，可发 `staged_plan_*` SSE。  
- `StepRunning { index, sub }`：第 `index` 步；`sub` 可选为「步内子状态」（见下）。  
- `PatchReplanner { attempt }`：`patch_planner` 模式下的重规划。  
- `DegradedToOuterLoop`：`no_task` 或规划解析失败降级到 **`run_agent_outer_loop`** 的路径已采纳。  
- `Done`：本分阶段回合结束。

**`StepRunning.sub`（可选子状态机）**：

- `BeforeStepLlm` / `InOuterLoop` / `AfterStepFailure` — 用于把「步失败是否 patch、是否继续」从深层 `if` 提升为显式转移，**需与** `staged_plan_feedback_mode`、`staged_plan_patch_max_attempts` **对齐**。

**实现侧对应**（代码演进，非一一命名的运行时状态变量）：`agent_turn/staged/step_iteration_fsm.rs` 中 **`StagedStepRunningSub`**（`AfterOuterLoop` 覆盖 transition、失败补丁、工具检查与成功收尾）；驱动函数见 **`staged/mod.rs`** 的 **`staged_step_run_outer_half`** / **`staged_step_run_after_outer_half`**。

**注意**：Hierarchical 模式的 Manager **不**强行走此 FSM；仅共享 **事件/效果** 的命名与日志规约，便于三模式对照。

---

## 4. 与现有类型与配置的对照

| 概念 | 现有落点 | FSM 中的角色 |
|------|----------|----------------|
| 终答是否需规划 | `FinalPlanRequirementMode` + `PlanRequirementSource` | 进入 Gate 的 Context，或合并为 `require_plan: bool` 只读 |
| 重写次数 | `plan_rewrite_attempts` / `plan_rewrite_max_attempts` | Gate 内计数或 `Exhausted` 转移条件 |
| 工作流层数/节点 | `last_workflow_validate_layer_count`、各 `validate_plan_*` | `CheckStructuredPlan` 的校验子步骤，不必单独成状态 |
| 分阶段失败策略 | `StagedPlanFeedbackMode`、`staged_plan_patch_max_attempts` | Staged Orchestrator 的转移表参数 |
| SSE 观测 | `AgentTurnSubPhase`、`staged_plan_*` 事件 | 转移时**顺带**发事件；`sub_phase` 仍与 **当前 LLM/工具阶段** 对齐，不必与 Gate 状态一一同名 |

---

## 5. 实现路线（建议顺序）

1. **文档与日志**：为 Gate 的「决策原因」增加结构化枚举（内部或 `debug!`），不先改控制流，便于与重构后 diff 行为。  
2. **终答 Gate 提取**：在 `per_coord` 子模块中新增 `final_plan_gate`（名可调整），**输入**与现 `after_final_assistant` 相同，**输出**仍为 `AfterFinalAssistant`；`after_final_assistant` 变为一行委托或薄包装。单测覆盖与当前行为矩阵一致。  
3. **分阶段 FSM（可选）**：在 `staged/mod.rs` 先抽取「从规划成功到 `send_staged_plan_started`」的转移，再步进 `for` 循环。  
4. **Hierarchical**：仅在 Manager 与 `PerCoordinator` 的**日志与 reason 枚举**上对齐，避免两套名词。

---

## 6. 非目标与风险

**非目标**：

- 不在本文档中定义新的 `agent_reply_plan` 版本。  
- 不替代 **`WorkflowReflectionController`** 的既有状态机；二者通过 **`PlanRequirementSource`** 衔接即可。  
- 不把 **`llm`** 或 **`tool_registry`** 拉进 FSM 层。

**风险**：

- 状态过多导致更难维护 —— **缓解**：严格限制状态个数，余量留在 Context。  
- 与侧向异步语义检查的顺序敏感 —— **缓解**：`PendingSemanticLlm` 单独状态，与现 `StopTurnPendingPlanConsistencyLlm` 路径一一对应。  
- 回归成本高 —— **缓解**：先行为对齐单测 + 重要集成路径；**禁止**在首版同时改用户可见 SSE 文本文案。

---

## 7. 修订记录

| 日期 | 说明 |
|------|------|
| 2026-05-02 | **`run_dispatch`**：**`execute_non_hierarchical_main_route`**；**`StagedPlanningDenyReason::as_str`** + **`staged_plan_intent_gate_deny_reason`**。**`outer_loop`**：**`OuterLoopIterationPhase`** + **`outer_loop_fsm`/`outer_loop_step`**（**`ReflectBranchCtl::as_trace_str`**）。**`agent_turn/errors`**：**`AgentTurnJobOutcomeKind`** + **`job_queue_*_outcome_kind`**；**`chat_job_queue`** 流式/JSON **`Err`** 分流。 |
| 2026-05-02 | **`workflow_reflection_controller`**：**`WorkflowReflectionFsmPhase`**、**`reflection_fsm_phase`**；`decide` 按相位分流；**`INSTRUCTION_WORKFLOW_REFLECTION_*`** 常量集中。 |
| 2026-05-02 | **`staged/prepared_parse_fsm`**：**`PreparedPlannerRoute`**、**`resolve_prepared_planner_route`**；**`run_staged_plan_with_prepared_request`** 首轮出口表驱动 + **`crabmate::staged`** **`prepared_request`** 观测；**`continue_prepared_plan_after_first_round`** 抽离 post-parse 分支。 |
| 2026-05-02 | **`staged/orchestrator`**：**`StagedRoundOrchestratorPhase`**（与 **`turn_fsm::StagedTurnPhase`** 区分）；**`run_staged_plan_steps_loop`** 增加 **`crabmate::staged`**（**`staged_fsm=steps_loop`**、**`steps_loop_phase`**）。 |
| 2026-05-02 | **`agent_turn/hierarchical_intent_route`**：**`HierarchicalPostIntentRoute`** / **`HierarchicalDiscourseFallbackReason`** + **`resolve_hierarchical_post_intent_route`**；**`hierarchy.rs`** 话语型回落路径写入 **`hierarchical_post_intent_route`** / **`hierarchical_discourse_fallback_reason`** 与回放 JSON。 |
| 2026-05-01 | **`agent_turn/intent/context.rs`**：**`build_intent_routing_context`** 统一装配 **`IntentContext`**，**`run_dispatch`** 与 **`intent/at_turn_start`** 共用。 |
| 2026-05-01 | **`docs/规划执行验证架构.md`** §2.5：分层 **`hierarchy`** 与 **PER/staged** 双轨职责表；源码索引补 **`hierarchy`** 入口。 |
| 2026-05-01 | **`agent_turn/hierarchy.rs`**：`run_hierarchical_agent` / **`handle_execution_result`** 增加 **`tracing`**（`target: crabmate::agent_turn`，**`hierarchical_phase`**）；与 **`turn_orchestration_mode=hierarchical`** 正交。 |
| 2026-05-01 | **`agent_turn/turn_orchestration`**：**`TurnOrchestrationMode`** + **`resolve_non_hierarchical_main_path`**；**`run_agent_turn_common` / `run_dispatch`** 打 **`tracing`**（`target: crabmate::agent_turn`，`turn_orchestration_mode`）。 |
| 2026-05-01 | **`agent_turn/reflect/reflect_semantic.rs`**：`PlanSemanticLlmOutcome` → **`PlanSemanticConsistencyReflectCtl`**（侧向语义 LLM 后与 **`final_plan_gate`** 挂起态衔接；单测覆盖）。 |
| 2026-05-02 | **`per_coord/final_plan_gate::run_final_plan_gate_semantic_completed`**：侧向语义 LLM 完成后的 **`PendingSemanticLlm`** 一步转移；**`reflect_semantic`** 并入 **`reflect_impl`**（单元测试随迁）。 |
| 2026-05-01 | **`after_final_assistant`**：**始终**经 **`run_final_plan_gate(phase, …)`**；`NoRequirement` 时不调用 **`workflow_validate_layer_need`**（避免无需求路径更新层数缓存）。**`outer_loop`**：`run_agent_outer_loop` 拆迭代守卫、上下文准备、**`ReflectBranchCtl`** 反思分支与工具执行轮。 |
| 2026-05-02 | 滚动视界外层实现迁至 **`staged/rolling_horizon_facade.rs`**（门面：`run_staged_rolling_horizon_outer_loop`、`run_staged_plan_then_execute_steps`、`run_logical_dual_agent_then_execute_steps`；删除 **`staged_rolling_horizon_outer_loop.inc.rs`**）；**`StagedPlanRunOutcome`** 仍留在 **`staged/mod.rs`** 避免 **`turn_fsm`** 与门面模块循环依赖。 |
| 2026-05-01 | 滚动视界外层：新增 **`staged_rolling_horizon_apply_advance`**（`turn_fsm.rs`），集中 **advance + rewrite 计数 + advance_kind / propagate_public_code**；`run_staged_rolling_horizon_outer_loop` 仅保留 IO 与 tracing。 |
| 2026-05-01 | 首轮解析后 **FullPipeline** 路径：新增 **`full_pipeline_fsm.rs`**（**`StagedFullPipelinePhase`** 线性相位 + `staged_fsm=full_pipeline` 的 `debug!`）；`run_staged_plan_with_prepared_request` 内 ensemble → 优化 → NL 段与枚举对齐。 |
| 2026-04-30 | 步内子阶段枚举 **`StagedStepRunningSub`**（`step_iteration_fsm.rs`，对齐设计稿 `StepRunning.sub`）；`mod.rs` 拆 **`staged_step_run_outer_half`** / **`staged_step_run_after_outer_half`** |
| 2026-04-30 | 步循环：`mod.rs` 内 **`run_one_staged_plan_step_iteration`**（单次迭代 I/O + `StagedStepIterationCtl`）；**`step_iteration_fsm.rs`** 增补墙钟 **`staged_step_wall_clock_exceeded`**、补丁反馈常量与 **`staged_step_verify_fail_patch_detail`** |
| 2026-04-30 | 步循环单次迭代（transition 之后）：`agent_turn/staged/step_iteration_fsm.rs`（outer_loop 后阶段划分、工具健康检查阶段路由）；与 **`step_loop_fsm`** / **`staged_step_fsm`** 并列 |
| 2026-04-30 | 解析成功后管线：`agent_turn/staged/post_parse_pipeline_fsm.rs`（ensemble/优化轮是否调用、结构化 `debug!`）；与 **`planner_round_fsm`**（路由枚举）配合 |
| 2026-04-30 | 步执行循环：`agent_turn/staged/step_loop_fsm.rs`（`transitions` 跳转、注入步 user）；**`staged_step_fsm.rs`**（补丁预算、`PatchPlanner`）；首轮解析 / `no_task` 历史：**`planner_parse_fsm.rs`**（`NotFound` 收敛 vs 降级） |
| 2026-04-30 | 逻辑多规划员内部：`agent_turn/staged/ensemble_fsm.rs`（辅助规划员草案采纳 vs 停止链、合并轮步列表应用）；与 `planner_round_fsm`（是否进入 ensemble）分层 |
| 2026-04-30 | 规划子回合门控：`agent_turn/staged/planner_round_fsm.rs`（ensemble / 优化轮是否运行）；与 `turn_fsm` / `orchestrator` 并列 |
| 2026-04-30 | 回合级：`agent_turn/staged/turn_fsm.rs`（`StagedTurnPhase` / `advance_staged_turn_after_sub_call`）；`prepare_messages_for_model` + `prepare_staged_planner_no_tools_request` fixture 测试见 `staged/mod.rs` |
| 2026-04-28 | 实现增量：`per_coord/final_plan_gate.rs`、`agent_turn/staged/orchestrator.rs` |
