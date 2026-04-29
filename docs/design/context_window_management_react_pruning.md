# 设计文档：上下文窗口管理（ReAct 循环裁剪）

**状态**：设计稿（待实现）  
**受众**：核心维护者、Agent 编排模块贡献者  
**关联文档**：`docs/PLAN_EXECUTE_VERIFY_ARCHITECTURE.md`、`docs/HIERARCHICAL_MULTI_CM_ARCHITECTURE.md`、`docs/design/agent_state_management.md`

---

## 1. 背景与问题

随着 ReAct 循环（Thought → Action → Observation）持续迭代，单次请求上下文会快速膨胀。当前风险主要体现在：

1. **上下文溢出**：历史消息和工具输出堆积，触达模型窗口上限。
2. **关键信息被稀释**：低价值历史占用预算，影响当前轮推理质量。
3. **状态漂移**：多次摘要后事实约束不一致，导致“记忆错位”。
4. **工具输出高噪音**：大段日志/JSON 原文注入，显著抬高 token 成本。

目标是在不损失任务连续性的前提下，建立可配置、可观测、可回放的上下文管理机制。

---

## 2. 设计目标与非目标

### 2.1 设计目标

1. **稳定控长**：每轮调用前都可预测地落在 token 预算内。
2. **任务连续**：保留可执行状态（目标、约束、待办、最近动作、关键证据）。
3. **高相关注入**：优先保留与当前目标强相关的历史片段。
4. **可审计**：裁剪前后可记录和追溯，方便调试回放。
5. **渐进接入**：与现有执行流兼容，可按配置逐步开启。

### 2.2 非目标

1. 本阶段不引入新的向量数据库依赖（仅预留接口）。
2. 本阶段不改变工具协议与消息协议（仅在准备消息阶段处理）。
3. 不在首版实现复杂多轮语义重写，仅提供结构化增量摘要。

---

## 3. 总体方案

### 3.1 四层上下文模型

将候选上下文分为四层，并定义不同裁剪策略：

1. **L0 系统层（Never Trim）**  
   系统提示、工具 schema、安全策略等，默认不裁剪。
2. **L1 任务层（Weak Trim）**  
   用户目标、约束、验收标准、显式偏好，弱裁剪。
3. **L2 工作层（Strong Trim）**  
   最近 ReAct 轮次、工具观测摘要、中间计划，强裁剪。
4. **L3 外部层（Retrieve Only）**  
   长日志、完整工具输出、历史长文本，仅在需要时检索回填。

### 3.2 每轮执行入口（Prepare Messages）

在发送模型前引入统一流程：

1. 统计预算（窗口总量、预留输出、输入上限）。
2. 组装候选上下文（L0~L3）。
3. 执行 ReAct 裁剪（硬保留、软压缩、重排、截断）。
4. 生成最终消息并写入“裁剪报告”用于观测。

---

## 4. Token 预算策略

### 4.1 预算公式

- 总窗口：`W`
- 预留输出：`R`
- 输入预算：`B = W - R`

建议默认预留：

- `R = max(2048, W * 0.2)`（可配置）

### 4.2 分桶预算（默认）

在 `B` 内按桶分配：

- `bucket_system`（L0）：20%
- `bucket_task`（L1）：20%
- `bucket_recent`（L2 最近原文）：30%
- `bucket_retrieval`（L3 回填）：30%

说明：

1. L0 预算不足时仅允许裁剪非关键附录，不裁核心规则。
2. L2/L3 预算可相互借用，但必须先满足 L1。

### 4.3 阈值驱动裁剪等级

- **< 70%**：不裁剪，仅常规组装。
- **70% ~ 90%**：压缩工具输出，保留最近 `k` 轮原文。
- **90% ~ 98%**：执行完整裁剪（摘要 + 重排 + 截断）。
- **> 98%**：降级模式（仅核心任务 + 最近 1 轮 + 必要证据）。

---

## 5. ReAct 循环裁剪算法

### 5.1 硬保留（Hard Keep）

无条件保留以下内容：

1. 当前用户输入。
2. 最近一轮完整 ReAct（Thought/Action/Observation）。
3. 未完成待办（todo）与当前子目标。
4. 已确认事实（facts）与不可违反约束（constraints）。
5. 最后一条失败原因（用于避免重复失败）。

### 5.2 软压缩（Soft Compress）

对历史片段执行结构化摘要，保留“可继续执行”所需最小信息：

- 历史轮次 → `state_summary`
- 超长 Observation → `observation_digest` + `log_ref`
- 工具 JSON → 关键字段白名单（id、path、status、error_code、counts）

### 5.3 相关性重排（Re-rank）

对候选片段评分：

`score = α * recency + β * relevance + γ * dependency + δ * evidence`

- `recency`：越新分越高
- `relevance`：与当前目标/子目标语义相关度
- `dependency`：是否被后续步骤依赖
- `evidence`：是否包含关键证据（错误码、路径、执行结果）

按分值降序填充预算桶，低分片段降级为引用键（不直接注入）。

### 5.4 最终截断（Final Trim）

若仍超预算，按顺序删除：

1. 低分 Observation 原文
2. 老旧 Thought 原文
3. 冗余 Action 描述

保留对应引用键，必要时可从外部层回放。

---

## 6. 数据结构（建议）

```text
ContextEnvelope
  - system_core: Vec<MessageChunk>
  - task_contract: TaskContract
  - working_set: Vec<ContextChunk>
  - retrieval_set: Vec<ContextChunk>
  - state_summary: StateSummary
  - trim_report: TrimReport

ContextChunk
  - chunk_id: String
  - source: ChunkSource
  - text: String
  - token_estimate: usize
  - score: f32
  - pinned: bool
  - replay_ref: Option<String>

TrimReport
  - before_tokens: usize
  - after_tokens: usize
  - dropped_chunk_ids: Vec<String>
  - compressed_chunk_ids: Vec<String>
  - mode: TrimMode
```

---

## 7. 关键模块拆分

### 7.1 `context_budgeter`

职责：

1. 估算 token 使用量。
2. 根据模型窗口与预留输出生成预算桶。
3. 输出裁剪等级（normal/soft/full/degraded）。

### 7.2 `context_summarizer`

职责：

1. 维护增量摘要（只更新 delta）。
2. 结构化记录：`goal / done / failed / next / facts`。
3. 定期触发主摘要重写（例如每 8 轮一次）。

### 7.3 `context_trimmer`

职责：

1. 执行硬保留、软压缩、重排、截断。
2. 生成 `TrimReport`。
3. 输出最终可发送消息序列。

### 7.4 `context_replay_store`

职责：

1. 存储被压缩/移出的完整原文。
2. 提供 `replay_ref -> full_text` 查询接口。
3. 支持按轮次/工具调用回放。

---

## 8. 与现有流程的集成点

建议在消息准备阶段接入（发送模型之前）：

1. 收集本轮候选上下文。
2. 调用 `context_budgeter` 判定预算级别。
3. 调用 `context_trimmer` 产出最终消息。
4. 记录 `TrimReport` 到日志与可选持久层。

集成收益：

1. 不影响工具执行路径。
2. 不改变外部 API 形态。
3. 可灰度开关，支持快速回退。

---

## 9. 配置项（建议）

```toml
[agent.context_window]
enabled = true
output_reserve_tokens = 2048
hard_keep_recent_rounds = 1
soft_keep_recent_rounds = 3
main_summary_rewrite_interval = 8

[agent.context_window.buckets]
system_ratio = 0.20
task_ratio = 0.20
recent_ratio = 0.30
retrieval_ratio = 0.30

[agent.context_window.thresholds]
soft_trim = 0.70
full_trim = 0.90
degraded = 0.98
```

---

## 10. 可观测性与评估

### 10.1 指标

1. `context_before_tokens` / `context_after_tokens`
2. `trim_drop_count` / `trim_compress_count`
3. `budget_mode` 命中分布
4. 单轮失败重试率（观察是否因裁剪导致信息缺失）
5. 任务成功率与平均轮数

### 10.2 日志建议

每轮写一条结构化日志：

- 当前预算级别
- 被删/被压缩 chunk id 列表
- 保留核心事实数量
- 是否触发降级模式

---

## 11. 风险与缓解

1. **摘要失真**  
   缓解：摘要模板固定结构；关键事实以原文引用保底。
2. **误删关键证据**  
   缓解：证据片段打 `pinned=true`；优先级高于普通历史。
3. **性能额外开销**  
   缓解：增量摘要 + 轻量 token 估算；只在阈值触发时全量处理。
4. **行为波动**  
   缓解：灰度发布与 A/B 指标对照，支持一键关闭。

---

## 12. 实施路线图

### Phase 1（MVP）

1. 引入 `context_budgeter` 与阈值分级。
2. 实现硬保留 + 最终截断。
3. 输出基础 `TrimReport`。

### Phase 2

1. 接入结构化增量摘要。
2. 工具输出白名单压缩。
3. 相关性评分重排。

### Phase 3

1. 引入外部回放存储与引用键。
2. 指标面板化与阈值在线调参。
3. 与长期记忆检索协同优化。

---

## 13. 验收标准

满足以下条件视为方案落地可用：

1. 常见任务场景下，上下文超窗错误明显下降。
2. 开启裁剪后任务成功率无显著回归（或提升）。
3. `TrimReport` 可稳定回放“删了什么、为何删除”。
4. 降级模式可在极端长上下文下保证任务继续执行。

