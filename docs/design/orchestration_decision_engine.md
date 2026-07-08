# 编排决策引擎：从二元门控到多因子评分

**状态**：设计稿（可分阶段落地）  
**受众**：维护者、Agent 架构贡献者、意图管线开发者  
**关联**：
- `crates/crabmate-agent/src/agent_turn/staged_planning_gate.rs` — 当前核心门控逻辑
- `crates/crabmate-agent/src/agent_turn/staged_planning_gate_types.rs` — 门控结果类型
- `crates/crabmate-agent/src/agent_turn/turn_route_decision.rs` — 路由决议与 `assess_turn_routing`
- `crates/crabmate-agent/src/agent_turn/turn_orchestration.rs` — `NonHierarchicalTurnResolution`
- `crates/crabmate-config/src/orchestration_profile.rs` — `OrchestrationProfile` 枚举
- `src/agent/agent_turn/intent/staged_planning_gate.rs` — 根包 L0+L1+L2 管线入口
- `src/agent/agent_turn/run_dispatch.rs` — 回合分发入口

---

## 1. 背景与问题

### 1.1 当前决策链路

```
用户消息
  → L0 预处理（关键词、文件路径、命令痕迹）
  → L1 快速规则路由（greeting / qa / execute 快速命中）
  → L2 细粒度意图分类（LLM，confidence + primary_intent + action）
  → staged_plan_eligibility_for_intent()  ← 核心门控
  → apply_orchestration_profile_to_staged_gate()  ← profile 覆盖
  → resolve_non_hierarchical_turn_phase()  ← 最终路由
```

### 1.2 门控现状

`staged_plan_eligibility_for_intent` 是**单条件二元判断**：

```rust
pub fn staged_plan_eligibility_for_intent(
    _task: &str,       // 未使用
    decision: &IntentDecision,
    _staged: &StagedPlanningConfig,  // 未使用
) -> Result<(), StagedPlanningDenyReason> {
    if !matches!(decision.action, IntentAction::Execute) {
        return Err(StagedPlanningDenyReason::IntentPipelineNotExecute);
    }
    Ok(())
}
```

`_task` 和 `_staged` 两个参数完全未使用。决策仅依赖 `IntentAction::Execute`。

### 1.3 问题

| 问题 | 说明 | 影响 |
|------|------|------|
| 分类粒度不足 | 不分任务复杂度，所有 Execute 类请求一律走 staged | 简单任务（如"运行 cargo build"）也触发规划轮，浪费 token 和延迟 |
| 上下文缺失 | 未利用工作区规模、任务长度、历史成功率等信号 | 决策信息不足，无法做精细化路由 |
| 不可观测 | 只有 Allow/Deny 日志，没有决策理由和因子得分 | 难以调优阈值和权重 |
| 不可扩展 | 新增因子需要修改核心判断逻辑 | 违反开闭原则 |
| 无反馈回路 | 决策结果不记录，无法学习优化 | 相同错误重复发生 |

---

## 2. 设计目标与非目标

### 2.1 目标

1. **多因子评分**：从单一条件升级为多因子加权打分，决策更精准。
2. **可解释**：每个决策附带因子得分，日志和 SSE 可展示决策理由。
3. **可配置**：因子权重、阈值可通过 TOML 配置，支持 A/B 测试。
4. **可扩展**：新增因子只需实现 trait，无需修改核心决策逻辑。
5. **渐进兼容**：Phase 1 行为与当前完全一致，后续阶段逐步引入新因子。

### 2.2 非目标

- 首版不引入在线学习（Phase 4 之后考虑）。
- 不改变分层（Hierarchical）路径的决策逻辑（分层有独立的路由器）。
- 不要求实时调整权重（权重变更需要 reload 配置或重启）。

---

## 3. 架构设计

### 3.1 总体架构

```
                        ┌──────────────────────────────┐
                        │     DecisionEngine            │
                        │                              │
  IntentDecision ──────►│  ┌────────────────────────┐  │
  Task + Messages ─────►│  │   FactorRegistry       │  │
  WorkspaceInfo ───────►│  │   (可扩展因子集)        │  │
  DecisionHistory ─────►│  │                        │  │
                        │  │  IntentFactor          │  │
                        │  │  ComplexityFactor      │  │
                        │  │  WorkspaceFactor       │  │
                        │  │  HistoryFactor         │  │
                        │  │  CostFactor            │  │
                        │  └───────────┬────────────┘  │
                        │              │               │
                        │              ▼               │
                        │  ┌────────────────────────┐  │
                        │  │   Scorer               │  │
                        │  │   Σ(weight × score)    │  │
                        │  │   threshold → Route     │  │
                        │  └───────────┬────────────┘  │
                        │              │               │
                        │              ▼               │
                        │  ┌────────────────────────┐  │
                        │  │   OrchestrationDecision │  │
                        │  │   route + confidence    │  │
                        │  │   + score_breakdown     │  │
                        │  └────────────────────────┘  │
                        └──────────────────────────────┘
                                      │
                                      ▼
                        NonHierarchicalTurnPhase
                        (Freeform | PlannedStep)
```

### 3.2 决策链路变更

```
当前：
  staged_plan_eligibility_for_intent(decision) → Allow/Deny
  → resolve_non_hierarchical_turn_phase() → Freeform/PlannedStep

目标（Phase 4 全量）：
  DecisionEngine::evaluate(decision, task, ctx, history)
    → OrchestrationDecision { route, confidence, breakdown }
  → 直接映射到 NonHierarchicalTurnPhase
```

### 3.3 因子设计

| 因子 | 权重 | 输入 | 输出 | 说明 |
|------|------|------|------|------|
| `IntentFactor` | 0.35 | `IntentDecision` | 0.0–1.0 | 意图置信度 + action 类型映射 |
| `ComplexityFactor` | 0.25 | 消息 token 数、多文件引用、需求数量 | 0.0–1.0 | 任务越复杂，staged 收益越高 |
| `WorkspaceFactor` | 0.20 | 项目文件数、语言、构建系统 | 0.0–1.0 | 大项目 staged 规划更有效 |
| `HistoryFactor` | 0.10 | 同类任务历史成功率 | 0.0–1.0 | 无历史时返回 0.5（中性） |
| `CostFactor` | 0.10 | 预估 token 消耗 vs 节省 | 0.0–1.0 | 小任务 staged 得不偿失 |

总分 = Σ(weight_i × factor_score_i)，范围 0.0–1.0。

### 3.4 阈值与路由映射

```
score < 0.4  → Freeform（外循环）
score ≥ 0.4  → Staged（分阶段规划）
```

`planner_executor_mode` 配置（`single_agent` / `logical_dual`）控制 Staged 模式下的模型分离策略，不由此引擎决策。

### 3.5 多意图处理

用户输入可能包含复杂意图或多意图：

| 输入 | 意图结构 |
|------|---------|
| "重构 auth 模块" | 单意图 |
| "重构 auth 模块并添加单元测试" | 多意图（并列） |
| "先修复登录 bug，然后优化数据库查询" | 多意图（有序） |
| "帮我看看这个错误，然后重构相关代码" | 多意图（依赖） |

当前 `IntentAction` 是单一枚举值，无法表达多意图的排列关系。设计方案：

#### 3.5.1 多意图信息（不修改 `IntentAction`）

`IntentAction` 是核心枚举，被意图管线、路由决议、门控等多处使用。为避免 breaking change，多意图信息作为独立字段存储在 `IntentDecision` 中：

```rust
/// 在 IntentDecision 中新增字段（不修改 IntentAction 枚举）。
pub struct IntentDecision {
    pub primary_intent: String,
    pub secondary_intents: Vec<String>,
    pub confidence: f32,
    pub action: IntentAction,          // 不变
    // ... 现有字段不变 ...

    /// 新增：多意图解析结果（仅 L2 填充，L0/L1 不做改造）。
    pub multi_intent: Option<MultiIntentInfo>,
}

pub struct MultiIntentInfo {
    pub item_count: usize,
    pub relation: IntentRelation,
}

pub enum IntentRelation {
    Parallel,
    Sequential,
}
```

#### 3.5.2 多意图对决策引擎的影响

多意图本身就是**强 staged 信号** — 多步骤任务天然适合分阶段规划。在 `ComplexityFactor` 中体现（Phase 2.5 启用，L2 提供多意图信息）：

```rust
impl DecisionFactor for ComplexityFactor {
    fn evaluate(&self, ctx: &FactorContext) -> FactorScore {
        let mut score = 0.0;

        // 多意图 → 主要加分信号（仅 L2 来源，Phase 2.5）
        if let Some(ref mi) = ctx.decision.multi_intent {
            score += 0.3 * (mi.item_count.min(5) as f32 / 5.0);
        }

        // 辅助信号（±0.1，微调，不独立驱动决策）
        score += token_count_secondary_signal(ctx);    // ±0.05
        score += file_reference_secondary_signal(ctx);  // ±0.05

        FactorScore { /* ... */ }
    }
}
```

#### 3.5.3 多意图来源

| 层级 | 方法 | 说明 |
|------|------|------|
| L2 | LLM 输出 `subtasks: [{kind, description}]` 和 `relation` | 唯一来源，Phase 2.5 实现 |

> **L0/L1 不做改造**：L0/L1 保持现有逻辑不变，不引入多意图检测。多意图识别完全由 L2 LLM 分类提供（Phase 2.5）。

#### 3.5.4 多意图路由策略

```
单意图 Execute  → 走评分引擎（各因子打分）
多意图（L2 检测）Parallel  → 最低 Staged（加权分 +0.2 保底）
多意图（L2 检测）Sequential → 最低 Staged（加权分 +0.3 保底）
```

多意图场景下 `Freeform` 路由基本排除（除非 `orchestration_profile = "freeform"`）。

#### 3.5.5 多意图因子调整

| 多意图类型 | IntentFactor 调整 | ComplexityFactor 调整 | 最低路由 |
|-----------|-------------------|----------------------|---------|
| 单意图 | 标准评分 | 标准评分 | 无限制 |
| 多意图 Parallel | `action` 权重 ×1.5 | +0.3×(N/5) | Staged（+0.2 保底） |
| 多意图 Sequential | `action` 权重 ×1.5 | +0.3×(N/5) | Staged（+0.3 保底） |

---

## 4. 核心类型定义

### 4.1 Factor trait

```rust
/// 决策因子：输入上下文，输出 0.0–1.0 的评分。
pub trait DecisionFactor: std::fmt::Debug {
    /// 因子唯一标识，用于日志和配置。
    fn id(&self) -> FactorId;

    /// 评估因子，返回 0.0（完全反对 staged）到 1.0（强烈建议 staged）。
    fn evaluate(&self, ctx: &FactorContext) -> FactorScore;

    /// 默认权重（可通过配置覆盖）。
    fn default_weight(&self) -> f32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactorId {
    Intent,
    Complexity,
    Workspace,
    History,
    Cost,
}
```

### 4.2 FactorContext

```rust
/// 因子评估所需的只读上下文。
pub struct FactorContext<'a> {
    pub decision: &'a IntentDecision,
    pub task: &'a str,
    pub messages: &'a [Message],
    pub cfg: &'a AgentConfig,
    pub workspace_file_count: Option<usize>,
    pub history: Option<&'a DecisionHistory>,
}
```

### 4.3 FactorScore

```rust
/// 单个因子的评估结果。
#[derive(Debug, Clone)]
pub struct FactorScore {
    pub factor: FactorId,
    pub raw_score: f32,       // 0.0–1.0
    pub weight: f32,           // 配置权重
    pub contribution: f32,    // raw_score × weight
    pub detail: String,       // 可解释的文本
}
```

### 4.4 OrchestrationDecision

```rust
/// 决策引擎输出（替代 StagedPlanningGateOutcome）。
#[derive(Debug, Clone)]
pub struct OrchestrationDecision {
    pub route: OrchestrationRoute,
    pub confidence: f32,
    pub score_breakdown: Vec<FactorScore>,
    pub total_score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestrationRoute {
    Freeform,
    Staged,
}
```

### 4.5 DecisionRecord（供数据分析）

```rust
/// 决策记录，持久化供后续分析（Phase 4 只写不读）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub task_preview: String,       // 脱敏截断
    pub primary_intent: String,
    pub decision: OrchestrationRoute,
    pub total_score: f32,
    pub breakdown: Vec<FactorScoreRecord>,
    pub outcome: Option<DecisionOutcome>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DecisionOutcome {
    Success { steps_completed: usize },
    PartialSuccess { steps_completed: usize, steps_total: usize },
    Failed { reason: String },
}
```

---

## 5. 配置设计

### 5.1 TOML 配置

```toml
[orchestration_decision]
# 决策模式："auto" | "scored"
# - auto: 保持现有行为（仅 IntentAction::Execute 判断）
# - scored: 使用多因子评分引擎（Phase 2+）
mode = "auto"

# 评分阈值（mode = "scored" 时生效）
staged_threshold = 0.4

# 因子权重（mode = "scored" 时生效，总和应为 1.0）
# 注意：以下为初始种子值，Phase 5 通过 golden set 评测调优。
[orchestration_decision.weights]
intent = 0.35
complexity = 0.25
workspace = 0.20
history = 0.10
cost = 0.10

# 反馈学习（Phase 4）
[orchestration_decision.learning]
record_decisions = true
```

### 5.2 环境变量

| 环境变量 | 说明 |
|----------|------|
| `CM_ORCHESTRATION_DECISION_MODE` | `auto` / `scored` |
| `CM_ORCHESTRATION_DECISION_STAGED_THRESHOLD` | staged 路由阈值 |
| `CM_ORCHESTRATION_DECISION_LEARNING_ENABLED` | 是否启用反馈学习 |

---

## 6. 渐进式实施方案

### Phase 1：架构搭建 + 行为不变（1–2 天）

**目标**：定义 trait 和类型，将现有逻辑迁移为第一个 `IntentFactor`，行为完全不变。

**变更**：
- 新增 `crates/crabmate-agent/src/agent_turn/decision_engine/` 模块
  - `mod.rs` — `DecisionEngine` 结构体 + `evaluate()` 入口
  - `factors.rs` — `DecisionFactor` trait + `FactorId` + `FactorScore`
  - `types.rs` — `OrchestrationDecision` + `OrchestrationRoute`
  - `intent_factor.rs` — `IntentFactor`（迁移现有 `staged_plan_eligibility_for_intent` 逻辑）
- `staged_planning_gate.rs` 中 `staged_plan_eligibility_for_intent` 改为调用 `DecisionEngine`
- 现有 `StagedPlanningGateOutcome` 和 `NonHierarchicalTurnResolution` 保持不变

**验证**：
- 现有单元测试全部通过
- 日志输出格式不变
- 路由行为与当前完全一致

### Phase 2：引入 ComplexityFactor（2–3 天）

**目标**：以 token 数和文件引用为信号，区分简单/复杂任务。

**变更**：
- 新增 `complexity_factor.rs` — token 数评分（消息长度估算）+ 文件引用计数
- 在 `mode = "scored"` 时启用多因子评分
- 添加配置项 `orchestration_decision.mode` 和 `orchestration_decision.weights`

**验证**：
- 简单任务（如 "cargo build"）得分低，走 Freeform
- 复杂任务（如 "重构 auth 模块"）得分高，走 Staged
- `mode = "auto"` 时行为不变

### Phase 2.5：多意图 L2 支持（1–2 天）

**目标**：L2 LLM 分类输出 `subtasks` 和 `relation`，作为多意图的唯一来源。

**变更**：
- L2 分类 prompt 增加 `subtasks` 和 `relation` 输出字段
- `IntentDecision` 新增 `multi_intent: Option<MultiIntentInfo>` 字段（不修改 `IntentAction`）
- `map_l2_candidate_to_decision` 填充 `multi_intent` 字段
- `FactorContext` 增加 `multi_intent` 字段（通过 `decision.multi_intent` 传递）
- `ComplexityFactor` 集成多意图信号
- 增加 L2 多意图单元测试覆盖

**验证**：
- L2 正确识别多意图并输出 subtasks 列表
- 多意图任务路由正确（最低 Staged）
- L2 未检测到多意图时行为不变

### Phase 3：引入 WorkspaceFactor + CostFactor（2–3 天）

**目标**：利用工作区元信息和成本预估优化决策。

**变更**：
- 新增 `workspace_factor.rs` — 项目文件数、语言、构建系统
- 新增 `cost_factor.rs` — 简单启发式：消息 token < 50 → 0.0，> 500 → 1.0，中间线性插值
- 完善 `FactorContext`，提供工作区信息

**验证**：
- 大项目正确偏向 staged
- 小项目/单文件任务偏向 freeform
- 阈值可配置，A/B 测试友好

### Phase 4：DecisionRecord 决策记录（2–3 天）

**目标**：记录决策和结果，积累数据供后续分析。（HistoryFactor 学习功能推迟到有足够数据后。）

**变更**：
- 新增 `decision_record.rs` — 持久化决策记录
- 决策记录存储到 SQLite（复用 `conversation_store_sqlite_path` 或独立路径）
- `outcome` 在回合结束时回写
- `history_factor.rs` 暂不实现，`HistoryFactor` 权重临时分配至其他因子

**验证**：
- 决策记录正确写入数据库
- 回合结束时 outcome 正确回写
- 记录不影响决策性能（异步写入）

### Phase 5：权重调优与 A/B 测试（持续）

**目标**：持续优化因子权重，建立评测基准。

**变更**：
- 建立决策评测数据集（golden set）
- 实现 `cargo run -- eval-decisions` 评测命令
- 权重热重载（`POST /config/reload`）

---

## 7. 文件结构

```
crates/crabmate-agent/src/agent_turn/
├── staged_planning_gate.rs          # 保留，内部调用 DecisionEngine
├── staged_planning_gate_types.rs    # 保留，渐进废弃
├── turn_route_decision.rs           # 保留，assess_turn_routing 适配新类型
├── turn_orchestration.rs            # 保留，NonHierarchicalTurnResolution 适配
│
└── decision_engine/                 # 新增模块
    ├── mod.rs                       # DecisionEngine 结构体 + evaluate()
    ├── traits.rs                    # DecisionFactor trait
    ├── types.rs                     # OrchestrationDecision, OrchestrationRoute, FactorContext
    ├── scorer.rs                    # 加权聚合 + 阈值判断
    ├── factors/
    │   ├── mod.rs                   # FactorRegistry
    │   ├── intent_factor.rs         # Phase 1
    │   ├── complexity_factor.rs     # Phase 2
    │   ├── workspace_factor.rs      # Phase 3
    │   └── cost_factor.rs           # Phase 3
    └── record.rs                    # DecisionRecord 持久化 (Phase 4)
```

---

## 8. 测试策略

### 8.1 单元测试

- 每个因子独立测试：给定输入 → 期望评分范围
- `Scorer` 聚合测试：多因子组合 → 期望路由
- 边界条件：`confidence = 0`、空任务、无历史数据
- 权重总和校验：`weights.sum() ≈ 1.0`

### 8.2 回归测试

- 现有 `staged_planning_gate` 测试全部保留并通过
- 新增 golden set 测试：预定义场景 → 期望路由
- `mode = "auto"` 时行为与当前完全一致

### 8.3 集成测试

- 端到端：用户消息 → 意图管线 → 决策引擎 → 正确路由
- 配置文件热重载：修改权重后决策变化

---

## 9. 废弃路径

- Phase 4 后，`StagedPlanningGateOutcome` 可标记 `#[deprecated]`
- Phase 5 后，`staged_plan_eligibility_for_intent` 可移除
- 门控相关拒绝原因（`StagedPlanningDenyReason`）逐步迁移到因子内部

---

## 10. 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| 因子权重不当导致 regression | 用户体感变差 | `mode = "auto"` 兜底，新因子在 `scored` 模式下渐进启用 |
| 复杂度因子误判 | 简单任务走 staged 浪费 token | 阈值保守设置（0.4），配合日志监控 |
| 决策错误无法 mid-turn 纠正 | Freeform 路由到复杂任务陷入多轮无效反思 | 后续考虑 Freeform 3+ 轮无进展自动切换 Staged；用户手动切换 |
| 决策记录膨胀 | 磁盘占用 | 限制记录条数（如最近 10,000 条），定期清理 |

---

## 11. 附录：当前代码引用

### 门控入口

```rust
// src/agent/agent_turn/intent/staged_planning_gate.rs:18
pub(crate) async fn assess_staged_planning_gate_full_pipeline(
    p: &mut RunLoopParams<'_>,
    sse_log_tag: &'static str,
) -> StagedPlanningGateOutcome { ... }
```

### 核心判断

```rust
// crates/crabmate-agent/src/agent_turn/staged_planning_gate.rs:26
pub fn staged_plan_eligibility_for_intent(
    _task: &str,
    decision: &IntentDecision,
    _staged: &StagedPlanningConfig,
) -> Result<(), StagedPlanningDenyReason> {
    if !matches!(decision.action, IntentAction::Execute) {
        return Err(StagedPlanningDenyReason::IntentPipelineNotExecute);
    }
    Ok(())
}
```

### Profile 覆盖

```rust
// crates/crabmate-agent/src/agent_turn/turn_route_decision.rs:302
pub fn apply_orchestration_profile_to_staged_gate(
    profile: OrchestrationProfile,
    intent_gate: &IntentGateSnapshot,
    gate: &StagedPlanningGateOutcome,
) -> StagedPlanningGateOutcome { ... }
```

### 最终路由

```rust
// crates/crabmate-agent/src/agent_turn/turn_orchestration.rs:85
impl NonHierarchicalTurnResolution {
    pub fn resolve(cfg: &AgentConfig, staged_gate: &StagedPlanningGateOutcome) -> Self {
        let allow_staged = staged_gate.allows_staged_planning();
        let turn_phase = resolve_non_hierarchical_turn_phase(cfg, allow_staged);
        ...
    }
}
```

---

## 12. 实施 TODO

### Phase 1：架构搭建（行为不变）

- [ ] **P1.1** 创建 `crates/crabmate-agent/src/agent_turn/decision_engine/` 模块目录
- [ ] **P1.2** 定义 `traits.rs` — `DecisionFactor` trait + `FactorId` 枚举
- [ ] **P1.3** 定义 `types.rs` — `OrchestrationDecision`、`OrchestrationRoute`、`FactorScore`、`FactorContext`
- [ ] **P1.4** 实现 `scorer.rs` — 加权聚合 `Σ(weight × score)` + 阈值路由
- [ ] **P1.5** 实现 `intent_factor.rs` — 迁移 `staged_plan_eligibility_for_intent` 逻辑
- [ ] **P1.6** 实现 `mod.rs` — `DecisionEngine` 结构体 + `evaluate()` 入口 + `FactorRegistry`
- [ ] **P1.7** 修改 `staged_planning_gate.rs` — `staged_plan_eligibility_for_intent` 内部调用 `DecisionEngine`
- [ ] **P1.8** 单元测试：Factor trait、Scorer 聚合、IntentFactor 行为一致性
- [ ] **P1.9** 回归测试：现有 `staged_planning_gate` 测试全部通过

### Phase 2：ComplexityFactor

- [ ] **P2.1** 实现 `complexity_factor.rs` — token 数评分（`task.chars().count() / 4` 粗略估算）+ 文件引用计数
- [ ] **P2.2** 配置项：`mode`、`weights`、`staged_threshold`
- [ ] **P2.3** 单元测试：ComplexityFactor 评分边界（空任务、短任务、长任务）
- [ ] **P2.4** 集成测试：简单任务 → Freeform，复杂任务 → Staged

### Phase 2.5：多意图 L2 支持

- [ ] **P2.5.1** `IntentDecision` 新增 `multi_intent: Option<MultiIntentInfo>` 字段（不修改 `IntentAction`）
- [ ] **P2.5.2** L2 分类 prompt 增加 `subtasks` 和 `relation` 输出 schema
- [ ] **P2.5.3** `map_l2_candidate_to_decision` 填充 `multi_intent` 字段
- [ ] **P2.5.4** `FactorContext` 增加 `multi_intent` 字段（通过 `decision.multi_intent` 传递）
- [ ] **P2.5.5** `ComplexityFactor` 集成多意图信号（+0.3×(N/5)）
- [ ] **P2.5.6** 单元测试：L2 多意图分类正确性
- [ ] **P2.5.7** 单元测试：L2 未检测到多意图时行为不变

### Phase 3：WorkspaceFactor + CostFactor

- [ ] **P3.1** 实现 `workspace_factor.rs` — 项目文件数评分
- [ ] **P3.2** 实现 `workspace_factor.rs` — 语言类型评分（Rust 项目偏 staged）
- [ ] **P3.3** 实现 `workspace_factor.rs` — 构建系统评分（Cargo/CMake 偏 staged）
- [ ] **P3.4** 实现 `cost_factor.rs` — 简单启发式：消息 token < 50 → 0.0，> 500 → 1.0，中间线性插值
- [ ] **P3.5** 完善 `FactorContext` 工作区信息字段
- [ ] **P3.6** 单元测试：大项目 vs 小项目评分差异
- [ ] **P3.7** 单元测试：单文件任务 cost 因子显著偏低

### Phase 4：DecisionRecord 决策记录

- [ ] **P4.1** 定义 `DecisionRecord` 和 `DecisionOutcome` 序列化类型
- [ ] **P4.2** 实现 `record.rs` — SQLite 存储（复用 `conversation_store_sqlite_path`）
- [ ] **P4.3** 实现 `record.rs` — 决策写入（回合开始时，异步）
- [ ] **P4.4** 实现 `record.rs` — 结果回写（回合结束时）
- [ ] **P4.5** 配置项：`learning.record_decisions`
- [ ] **P4.6** 单元测试：DecisionRecord 读写正确性
- [ ] **P4.7** 单元测试：记录不影响决策性能

### Phase 5：权重调优与 A/B 测试

- [ ] **P5.1** 建立 golden set 决策评测数据集（≥ 50 条场景）
- [ ] **P5.2** 实现 `cargo run -- eval-decisions` 评测命令
- [ ] **P5.3** 权重热重载支持（`POST /config/reload`）
- [ ] **P5.4** 决策日志结构化输出（JSON 格式，含因子得分）
- [ ] **P5.5** SSE 事件：决策理由展示给前端
- [ ] **P5.6** 文档：因子调优指南