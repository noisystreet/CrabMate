# 观众角色（侧向点评）：设计草案

**状态**：设计稿（**未**承诺实现时间表；默认**不**进入当期迭代承诺）。  
**受众**：维护者、产品与协议设计者。  
**语言**：中文。  
**维护**：实现或废弃某能力时，同步修订本文，并更新 **`docs/未来规划功能.md`** / **`docs/en/FUTURE_PLANS.md`** 中的索引段落。

**关联文档**：

- **`docs/规划执行验证架构.md`**：P–E–V 与 **`plan_rewrite` / `workflow_reflection` / `final_plan_semantic_check`** 的职责边界。  
- **`docs/开发文档.md`**：`agent_turn` / `per_coord` / `staged` / `llm::complete_chat_retrying` 调用约定。  
- **`docs/SSE协议.md`**：若新增控制面事件，须与 **`frontend-leptos`** 及 **`crates/crabmate-sse-protocol`** 对齐。  
- **`.cursor/rules/secrets-and-logging.mdc`**：侧向请求的摘要与日志脱敏为**硬性**约束。

---

## 1. 背景与动机

主 Agent 在**规划（P）—执行（E）—反思 / 重试（R）**链路上可能产生：规划与工具事实不一致、步间假设漂移、反思后仍遗漏风险等情况。已有能力包括：

- **确定性验证**：分阶段 **`steps[].acceptance`** + **`step_verifier`**；分层 **`GoalVerifier`** 等。  
- **终答形态与重写**：**`final_plan_requirement`** + **`plan_rewrite_max_attempts`**（**`per_coord::after_final_assistant`** / **`final_plan_gate`**）。  
- **可选侧向一致性**：**`final_plan_semantic_check_*`**（**`per_plan_semantic_check`**）：在特定条件下对「终答规划 vs 近期工具摘要」做一次**无工具**短调用，默认 **fail-open**。

**观众角色**指在编排层增加的、**不直接调度工具**的**可选**侧向模型调用：对**已发生的**规划 / 执行 / 反思片段给出结构化分析与评价，用于**可观测性**、**可选**驱动重写或补丁、以及后续**离线**调参（不隐含「自动训练模型权重」）。

---

## 2. 术语与范围

| 术语 | 含义 |
|------|------|
| **观众（critic / audience）** | 编排层发起的**额外** `chat/completions` 调用（**无工具**），输入为**服务端构造的摘要**，输出为固定或半结构化点评。 |
| **`agent_roles` 多角色** | 会话主 Agent 的**人设与工具白名单**；**不是**本设计中的「观众」同义词。观众可为**同一模型**或**独立模型配置**（若将来引入独立配置键，须在 `AgentConfig` 与文档中单独声明）。 |
| **与 `final_plan_semantic_check` 的关系** | 语义上可重叠；若同时存在，必须在实现中定义**互斥、顺序或短路**规则，避免对同一锚点**重复**侧向调用（见 §6）。 |

**不在本设计首版范围内**（可列为后续阶段）：多进程独立「评委服务」、用户自定义任意长 prompt 注入未脱敏 tool 原文、无上限的每步嵌套点评。

---

## 3. 设计目标与非目标

### 3.1 目标

1. **可观测**：在 tracing / 可选 SSE 中暴露「何时点评、点评结论类别、是否参与闭环」。  
2. **有界**：每回合 / 每分步的调用次数、`max_tokens`、墙钟预算可配置且有**硬上限**。  
3. **安全默认**：侧向请求与日志遵守脱敏与截断；**默认 fail-open**（侧向失败不阻断主循环），除非显式开启「强闸门」类实验模式并单独文档化风险。  
4. **与现有闭环正交**：不弱化 **`acceptance`** / **`step_verifier`** 的确定性地位；观众输出为**补充信号**。

### 3.2 非目标

- **替代**确定性验收或 **`plan_rewrite`** 的静态规则。  
- **保证**点评正确（LLM-as-judge 的误判需在 §9 缓解与评测中处理）。  
- 在首版承诺**全路径**（分层 `hierarchy` + 非分层 `staged` + 纯 `outer_loop`）行为完全一致；允许分轨分阶段落地，但须在文档与 **`turn_orchestration_mode`** 可观测字段上写清覆盖范围。

---

## 4. 触发策略（建议枚举，实现时固化为配置）

以下触发点**可多选组合**，默认建议**全部关闭**，仅开启少数场景。

| 锚点 | 说明 | 信息可用性 |
|------|------|------------|
| **A. 分阶段每步结束后** | 当前步 outer 完成、进入下一轮规划前 | 本步 user 注入、本步 tool 结果摘要 |  
| **B. 补丁规划合并后** | **`patch_planner`** 成功合并 `steps` 后 | 新旧步骤 diff 摘要 |  
| **C. 终答规划静态通过后、语义检查前/后** | 与 **`final_plan_semantic_check`** 强相关 | 须定义与现有侧向调用的**先后或合并** |  
| **D. 工作流反思链关键节点** | **`WorkflowReflectionController`** 决策变更后 | 仅 workflow 路径 |  
| **E. 分层子目标验证失败后** | 与 **`reflect_and_replan`** 相邻 | 与 PER 轨分离计数，避免混称 `plan_rewrite` |

**节流**：同一锚点在短时间内重复触发时，应支持「合并摘要 / 跳过冗余点评」策略（具体阈值配置化）。

---

## 5. 输入构造原则

1. **禁止**在侧向 `messages` 中放入完整 **`API_KEY`**、完整 **`Authorization`**、可复原的密钥片段或未脱敏的 tool 原始大段输出。  
2. 摘要应复用或对齐 **`per_plan_semantic_check`** / **`summarize_messages_for_final_plan_semantic_check`** 的**信封化**思路：`summary`、`ok`、短预览、条数上限。  
3. 与 **`message_pipeline` / `context_window`** 的关系：侧向请求的 token **不计入**主对话窗口的误解风险需在实现注释中说明；若共享同一 **`max_turn_duration_seconds`**，需明确是否单独子预算或并行（推荐：**串行**且占用墙钟，避免与取消语义冲突）。  
4. **唯一入口**：侧向 HTTP 仍须经 **`llm::complete_chat_retrying`**（见 **`docs/开发文档.md`**「`agent_turn` 与 `llm`」）。

---

## 6. 输出契约（建议）

优先**机器可读** JSON（便于后续接重写与金测），人类可读说明放在固定字段内。

**建议最小 schema（示例名，实现时可调整）**：

```json
{
  "type": "crabmate_audience_review",
  "version": 1,
  "verdict": "pass|warn|fail",
  "dimensions": {
    "plan_soundness": "ok|concern|bad",
    "tool_alignment": "ok|concern|bad",
    "risk": "low|medium|high"
  },
  "bullet_points": ["短句1", "短句2"],
  "suggested_actions": ["optional actionable hint"]
}
```

- **`verdict: fail`** 是否自动触发 **`plan_rewrite`** 或分阶段补丁：**默认否**；若开启，必须占用**独立计数器**或与现有计数关系在 **`GET /status`** 镜像中写清。  
- **解析失败**：视为 **`pass`** 或 **`warn`**（与 **`final_plan_semantic_check`** 的 fail-open 一致），并打 **`debug`** 级别结构化日志。

---

## 7. 与主循环的集成要点

| 现有机制 | 观众层须对齐的点 |
|----------|------------------|
| **`plan_rewrite_attempts`** | 若观众驱动终答重写，是否递增、是否与静态门控共享上限 |  
| **`staged_plan_patch_planner_rounds_completed`** | 若观众仅影响补丁文案而不合并步骤，不得误增补丁轮次 |  
| **`WorkflowReflectionController`** | 注入类指令与常量并列文档化，避免与 **`workflow_reflection_plan_next`** 字符串散落耦合 |  
| **`hierarchy::reflect_and_replan`** | 分层轨与 PER 轨分离；观众若仅支持一线，须在 **`turn_orchestration_mode`** 或等价日志中可区分 |

---

## 8. 可观测性与协议

1. **Tracing**：统一 `target`（建议 `crabmate::audience` 或与 `crabmate::per` 并列的子 span），携带 `conversation_id` / `job_id` / `plan_id` / `step_id`（若可得）。  
2. **SSE（若需要 UI）**：新控制面事件须更新 **`docs/SSE协议.md`**、**`frontend-leptos/src/sse_dispatch.rs`**、**`fixtures/sse_control_golden.jsonl`** 与 **`crates/crabmate-sse-protocol`**（见仓库 **api-sse-chat-protocol** 维护清单）。  
3. **会话消息**：默认建议**旁路**（仅日志 + SSE），**不**写入上送模型的 `messages`；若产品要求写入时间线，应使用与现有「分阶段旁注 system」一致的模式，并评估 token 膨胀。

---

## 9. 风险与缓解

| 风险 | 缓解 |
|------|------|
| 误判导致无意义重写 | 默认不联动重写；联动时使用高置信度门限 + 独立上限 |  
| 成本与延迟 | 默认关闭；锚点稀疏；`max_tokens` 上限；与墙钟预算协调 |  
| 提示注入 / 泄露 | 摘要-only、长度截断、密钥与路径策略与全局规则一致 |  
| 与语义检查重复 | 配置枚举「仅观众 / 仅 semantic_check / 合并单次调用」 |

---

## 10. 分阶段落地建议

| 阶段 | 内容 |
|------|------|
| **MVP** | 单一锚点（例如「分阶段每步结束」）、仅 tracing + 可选文件日志、结构化 JSON、默认 fail-open、**不**改 SSE |  
| **M2** | 可选 SSE 事件 + Web 时间线展示；配置键与 **`POST /config/reload`** 边界文档化 |  
| **M3** | 可选联动 **`plan_rewrite`** 或补丁 user 注入；与 **`final_plan_semantic_check`** 合并或编排优化 |  
| **M4** | 分层路径对齐；离线评测集与 CI 钩子等 |

---

## 11. 验收与回归

- **单元**：JSON 解析、verdict 映射、计数器不与 `plan_rewrite` 误耦合。  
- **集成**：关闭观众时零额外 HTTP；开启时 wiremock 或 fixture 测一次完整锚点。  
- **协议**：若引入 SSE，跑 **`cargo test golden_sse_control`**（或仓库当期等价命令）。

---

## 12. 配置草案（占位，非实现承诺）

实现时应在 **`config/`** 与 **`docs/配置说明.md`** 中给出正式键名与范围。草案级思路：

- 总开关：`audience_enabled`（默认 `false`）。  
- 锚点掩码：`audience_triggers`（枚举集合）。  
- 预算：`audience_max_calls_per_turn`、`audience_max_tokens`、`audience_timeout_ms`（数值须与 **`finalize`** 校验策略一致）。  
- 环境变量前缀建议 **`CM_AUDIENCE_*`**，与现有 **`CM_FINAL_PLAN_SEMANTIC_CHECK_*`** 并列，避免含义混淆。

---

*本文档为规划层共识；具体键名、SSE 负载与默认以落地时的 `README.md` / `docs/配置说明.md` / `docs/SSE协议.md` 为准。*
