# 设计文档：完成判定的第 ② 层（LLM 语义判定）

**状态**：设计草案（#1387 后续阶段）。**语言**：中文。

**跟踪**：[#1387](https://github.com/noisystreet/CrabMate/issues/1387)（S0.5 完成判定层重建）、
[#1386](https://github.com/noisystreet/CrabMate/issues/1386)（路线级）。

**关联文档**：

- [`agent_autonomy_enhancement.md`](./agent_autonomy_enhancement.md)：路线级（§3.2 实测缺陷、§4 L1 两层架构）。
- [`subagent_delegation.md`](./subagent_delegation.md)：§3 能力 trait 注入范式（本文 §6 照此形状）。
- [`multi_agent_collaboration.md`](./multi_agent_collaboration.md)：§0 三条硬原则（契约不承载内部表示 /
  验收必须可判定 / 裁决链唯一），本文 §2 / §4 / §5 直接遵循。
- [`run_loop_state_ownership.md`](./run_loop_state_ownership.md)：回合可变状态归属。

---

## 1. 目标与非目标

### 1.1 目标

第 ① 层（已落地，`task_level_evidence/evidence.rs`）只回答「发生了什么」。它对**综合任务**判不了：

- 「重构 X 并保证行为不变」——`cargo test` 通过算不算？
- 只读分析类任务——「读过文件」与「已给出分析」无法从结构化证据区分。

第 ② 层回答「**这些事实够不够算完成**」，并满足三条通用性要求：

1. **后端无关**：输入是结构化 DTO，不是 `messages`（§0-1 教训：MiniMax 网关会合 `system`/`user`）。
2. **可确定性测试**：判定器可注入，单测与金样不碰真模型。
3. **判定与处置分离**：判定层产出数据，编排层决定行为。

### 1.2 非目标

- **不**硬要求「判定模型 ≠ 执行模型」。单模型部署（本地 Ollama / 单 model 网关）必须可用。
  自评偏差靠 §4 的锚点约束缓解，不靠换模型。
- **不**在判定层做「继续 / 停止」决策。那是编排层的产品偏好，硬编码会让判定层耦合场景。
- **不**做二次 LLM 校验「证据是否真的支持结论」。两级 LLM 级联会叠加延迟、放大错误、且无法金样。
- **不**替代第 ① 层。结构化证据仍优先：能确定判定的一律不调 LLM。

---

## 2. 契约

### 2.1 输入

```rust
/// 判定输入。**刻意不含 `messages`**：那是后端内部表示，跨后端不稳定。
pub struct CompletionJudgeInput {
    /// 用户任务原文（最新一条真实 user 消息）。
    pub task: String,
    /// 第 ① 层采集的结构化证据（已按字符上限截断）。
    pub evidence: Vec<JudgedEvidence>,
}

pub struct JudgedEvidence {
    pub id: String,          // ev_1…
    pub kind: String,        // verify / write / other
    pub strength: String,    // strong / weak
    pub tool: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub detail: String,
}
```

**不含 assistant 自述**：切断「模型给自己的结论打分」的回环。这是 §4 锚点机制生效的前提。

### 2.2 输出

```json
{ "status": "satisfied" | "partial" | "unsatisfied",
  "evidence_refs": ["ev_3", "ev_7"],
  "missing": ["未验证边界条件"] }
```

对应 Rust：

```rust
pub enum CompletionStatus { Satisfied, Partial, Unsatisfied }

pub struct CompletionVerdict {
    pub status: CompletionStatus,
    pub evidence_refs: Vec<String>,
    pub missing: Vec<String>,   // 截断到上限
    /// 本次结论是**锚点校验降级**来的（模型原本给 `satisfied`，见 §4），而非模型自己判否。
    pub degraded: bool,
}
```

`missing` 为空时序列化省略；`evidence_refs` 上限 16 条、`missing` 上限 8 条、单条 120 字符。

`degraded == true` **只可能与** `unsatisfied` **同现**：§4 的降级只作用于 `satisfied`，
`partial` 不受锚点校验影响。终态层据此区分「引用格式不合规」与「任务真没做完」——只有后者
才把 `stream_ended.reason` 记成 `partial`（#1472）。若将来锚点校验开始改写 `partial`/
`satisfied` 以外的状态，`chat_job_queue::stream_finish::verdict_marks_incomplete` 与本节需同步。

---

## 3. 判定与处置分离（决策 Q2）

**判定层只产出 `CompletionVerdict`，不决定停不停。** 编排层按**资源约束**（而非产品偏好）处置：

| `status` | 有剩余预算 | 预算耗尽 |
|---|---|---|
| `satisfied` | 允许早停 | 允许早停 |
| `partial` | 继续，把 `missing` 注入下一轮 | 停，把 `missing` 交用户 |
| `unsatisfied` | 继续 | 停 |

为什么用「预算余量」而非「场景」：这条规则不依赖交互式/后台、单 Agent/委托，换场景不用改判定层。

---

## 4. `evidence_refs` 校验：只做机器可校验的部分（决策 Q3）

`status = satisfied` 必须同时满足：

1. **`evidence_refs` 非空**，且每个 id 都在本次 `EvidenceSet` 中（结构校验）。
2. **引用中至少一条是 `verify` 或 `write` 类型**——挡「引了一堆只读探查就宣称完成」。

任一不满足 → **降级为 `unsatisfied`**，并在 `missing` 里写明降级原因。

这条替代了不可判定的「语义是否支持」问题：用**证据种类约束**做代理，零成本、可确定性测试。
至于「证据是否真的支持结论」，属评测问题，交给 benchmark 与人工，**不进运行时**。

---

## 5. 可注入判定器（可测试性硬要求）

LLM 判定天生不确定。若直接嵌进判定路径，**金样会随模型波动**，CI 不可靠，后续无人敢改这块。

因此判定器是 trait：

```rust
#[async_trait]
pub trait CompletionJudge: Send + Sync {
    async fn judge(&self, input: CompletionJudgeInput) -> Result<CompletionVerdict, ToolError>;
}
```

- 生产实现照 `per_plan_semantic_check.rs`：JSON-only、短 prompt、`max_tokens` 钳制、fail-open。
- 单测 / 金样用 mock 实现（返回固定 verdict，含越权 `evidence_refs` 以验证 §4 降级）。
- 真模型集成测试单独一组，默认不跑。

---

## 6. 调用点与 fail-open

**已接线**（`turn_completion::turn_early_stop_allowed_with_judge`，由 `outer_loop` 在工具批后 `await`）。

**只在候选终止点调用**，一回合一次（不按工具批）：

1. 模型给出终答（无 `tool_calls`）时 —— 与 `per_reflect_after_assistant` 同一时刻。
2. 预算阈值处 —— 决定「带半成品停」还是「再给一轮」。

**不再有「第 ① 层触发前置」（#1415 已删第 ① 层）**：原先的分级是「结构化证据先给
`Satisfied` / `Missing` / `NotApplicable`，只有最后一个才问 LLM」。领域检测器（只有 C++ 一个、
本身也是文本启发式）与通用关键词兜底一并移除后，**早停门只有一个判定源**：

- `completion_judge_enabled` 且未超调用上限 → 直接问第 ② 层；
- 判定层不可用（开关关 / 无 `API_KEY` / LLM 失败 / 回复无法解析 / 无可用窗口）→ **不早停**。
- `satisfied` 但锚点校验未过 → `judge_and_validate` 已降级为 `unsatisfied`，同样不早停。

**代价（有意接受）**：早停能力现在**完全依赖 LLM 可用性**；关掉开关时任何任务都不会提前停轮。

**成本护栏（两道，缺一不可）**：

早停门在**每个工具批之后**都会走一次，每次都有新证据（指纹变化）。实测：5 轮工具批
会产生 **5 个不同指纹**，所以只做指纹去重仍会 5 次调用。

1. **每回合硬上限** `MAX_COMPLETION_JUDGE_CALLS_PER_TURN = 3`（`PerTurnCounters`，随回合重置）；
2. **证据指纹去重**（条目数 + 最后一条 id/成败）：同一份事实只问一次。

实测结果：5 轮工具批 → **3 次**判定调用（上限生效）。没有这两道闸，50 轮任务会发 50 次。

取 3 的理由：早停若真有价值，通常前几轮就能判出；任务刚开始时即使判出完成也只省一两轮，
收益远小于一次 LLM 往返。需要更大额度时再升为配置键。

**fail-open**：LLM 失败 / 超时 / JSON 解析失败 → **不早停**（绝不因判定失败而早停）。

**处置由编排层负责**（§3）：判定层只回 `status`，本接线点当前只把 `satisfied` 映射为「允许早停」，
其余一律「继续」。预算耗尽时的「带半成品停」由 `turn_budget` 的既有中止路径处理，不在早停门内做。

---

## 7. 配置面

新开一组，**不复用** `final_plan_semantic_check_*`：两者是不同消费者（规划一致性 vs 完成判定），
共用开关会导致「想开一个必须开两个」。形状对齐既有组。

```toml
completion_judge_enabled = true         # 默认开（#1415）；关 = 构建/测试与只读分析一律不早停
completion_judge_max_tokens = 512       # 钳制 32..=1024
completion_judge_model = ""             # 空 = 继承默认；仅覆盖，不强制与执行模型分离
```

需同步：`config/default_config.toml`、`docs/配置说明.md`、（如进 SSE）`docs/SSE协议.md`。

---

## 8. 风险

| 风险 | 缓解 |
|---|---|
| 假阳性（判完成但没完成）→ 提前终止交半成品 | §4 锚点硬约束 + fail-open 只退不放 + 金样覆盖三类任务 |
| 判定模型顺同偏差 | 输入剔除 assistant 自述；`completion_judge_model` 可选覆盖（非强制） |
| 成本 / 延迟 | 仅候选终止点 + `max_tokens ≤ 1024` + 每回合 ≤3 次 |
| 金样随模型漂移 | §5 可注入 trait，单测金样用 mock |
| 早停完全依赖 LLM 可用性（#1415 删第 ① 层后） | 无确定性短路：关掉开关 / 无 `API_KEY` / 判定失败时**任何任务都不早停**，回合跑到迭代上限 |
| fail-open 只退不放 | 判定失败绝不导致早停；不需要「与第 ① 层结论冲突」的仲裁（已无第 ① 层） |

---

## 9. 验收

- 综合任务：「重构 X 并保证行为不变」+ `cargo_test` 通过 → `satisfied` 且 `evidence_refs` 指向该验证证据。
- 锚点约束：mock 返回 `satisfied` 但 `evidence_refs` 为空 / 只引 `other` → **降级为 `unsatisfied`**。
- fail-open：mock 返回错误 → **不早停**（不存在「第 ① 层结论」可维持了）。
- 关闭时：`completion_judge_enabled = false` 时**零 LLM 调用**，且**任何任务都不早停**（#1415：已无检测器兜底）。
- `pre-commit run --all-files` 全绿；`cargo test -p crabmate --lib` 全绿。

---

## 10. 决议记录

| 决策 | 结论 | 依据 |
|---|---|---|
| 判定模型选型 | `Option` 默认继承，可覆盖，**不硬要求分离** | 硬约束会让单模型部署不可用；正确性靠 §4 锚点而非换模型 |
| `partial` 处置 | **判定/处置分离**，按预算余量 | 产品偏好不该进判定层；资源约束天然通用 |
| `evidence_refs` 严格度 | **id 存在 + 种类约束**，无二次 LLM | 二级级联叠加延迟、放大错误、无法金样 |
