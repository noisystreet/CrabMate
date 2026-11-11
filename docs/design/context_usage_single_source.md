# 回合输入用量的单一来源（#1347 S3 设计）

> 状态：**S3a 已实施**（`TurnInputUsage` + 三出口收敛到单点取数，取值口径未变）；
> **S3b 未实施**（出口取值口径拆分 + `/status` 补 `context_budget`，需 Client 同步）。
> 对应 Issue #1347 的 S3（S1 characterization golden、S2 决策 policy 已落地）。
> 本文只解决「用了多少 Token 由谁定义」；压缩**判定**见 [context_window_compaction.md](context_window_compaction.md)，
> 事件**形状**见 [context_timeline_detail.md](context_timeline_detail.md)。
>
> 基线：`main` @ `69176f0d`（S2 已合入 main）。

## 1. 问题

issue 的原始描述是「四处各算一套」。核对代码后，更准确的说法是：**同一份数据被塞进语义不同的字段**，
而展示层要靠一个字符串标记去猜它是哪一种。

三个具体矛盾：

1. **`prompt_tokens` 名不符实**。`enrich_snapshot_from_latest_model_context_artifact`
   （[tiktoken_prompt_tokens.rs](../../src/agent/tiktoken_prompt_tokens.rs)）把 `prompt_tokens` 覆写成
   `report.after.message_tokens`（**不含** tool schema，默认配置下差约 29K），而同一条 snapshot 里
   `used_input_tokens` 却是含 schema 的总量。两个字段名并存，读者无法判断哪个是「模型实际收到的输入」。
2. **`used_input_tokens` 一字段两语义**。同一处用
   `provider_input_tokens.unwrap_or(estimated)` 赋值，再靠 `counting_source = "provider_usage"` 这个字符串
   区分。Client 的用量水位（`crabmate-client-api` 的 `context_axis.rs`，`ContextUsedWatermark`）据此做
   「滚动最大 / 压缩后回落」，两种口径混用会让状态栏在「估算」与「上游计费」之间跳变。
3. **同回合两出口可能不等**。`context_trim.detail.after_tokens` 恒为 tiktoken 估算
   （[context_timeline/mod.rs](../../src/cm_agent/context_timeline/mod.rs) 的 `ContextCompactionTimelineSnapshot`），
   而 SSE `tiktokenPromptTokens.used_input_tokens` 在同回合可能已是 provider 值。

另外两个事实决定了验收怎么写：

- `GET /status`（[health_status.rs](../../src/web/chat_handlers/health_status.rs)）**没有**回合用量，
  只有配置、新会话基线估算（`tiktoken_new_session_baseline_by_agent_role`）与进程累计计数器。
- `crabmate doctor`（[cli_doctor.rs](../../src/runtime/cli_doctor.rs) 的 `doctor_context_gates_lines`）
  是**离线**命令，没有回合，只能给预算推导。issue 验收里「doctor 也给同一个 `used`」不成立。

## 2. 决策

### 2.1 权威值对象

在 `cm_types` 新增（与 `TiktokenPromptTokensSnapshot` 同族，SSE / HTTP 共用，不含 IO）：

```rust
pub struct TurnInputUsage {
    /// 视图口径：估算的最终请求输入，含 tool schema / 附件 / vendor 开销。
    /// 「本回合用了多少」只认这一个；它永不被 provider 值覆写。
    pub estimated: ContextTokenEstimate,
    /// 计费口径：上游 usage 回填。独立字段，可缺省。
    pub provider_input_tokens: Option<u64>,
    /// 本回合预算（窗口 / 输入上限 / trigger / target）；未配置窗口时为 None。
    pub budget: Option<ContextTokenBudget>,
}
```

- **唯一产点**：`TurnInputUsage::from_report(&ContextCompactionReport)`。
  `ModelContextArtifact.compaction` 是既有唯一真相（回放、归档、审计都用它），本改造不新增数据源，
  只是给它一个语义明确的对外形状。
- 两条口径**并列**而非互斥：`estimated` 恒有值，`provider_input_tokens` 有就填、没有就 `None`。
  消费方要哪个取哪个，不再靠 `counting_source` 字符串反推。

### 2.2 各出口职责

| 出口 | 改造 |
| --- | --- |
| `context_trim.detail` | `after_tokens = estimated.used_input_tokens`（字段名与值均不变）；新增 `provider_input_tokens`（有值才出现） |
| SSE `tiktokenPromptTokens` | `used_input_tokens` **只填 estimated**；`provider_input_tokens` 独立字段；`prompt_tokens` 恢复为「总输入估算」而不是被覆写成 message-only（**破坏性**，见 §4） |
| `GET /status` | 新增 `context_budget`（静态快照：`context_window_tokens` / `max_input_tokens` / `trigger_tokens` / `target_tokens`），供 Client 取分母。**不加入回合用量**（见 §2.3） |
| `crabmate doctor` | 不改数值，只改文案：明确「预算推导 + 基线估算，非回合实测」，并在文档里声明它不是 usage 出口 |

### 2.3 为什么回合用量不进 `GET /status`

`/status` 是无上下文的 GET。要塞进「本回合用量」就得建「按 `conversation_id` 的最近 usage 缓存」，
并承担与 SSE 双写不一致、进程重启丢失、并发会话互相覆盖三类问题；而 SSE 已经有这条通道。

除非需要「切回旧会话时状态栏立刻有数」这一产品能力——那是一个独立特性（需要持久化 usage），
不属于 S3。**本设计明确不做**，并在 `/status` 的字段说明里写清它不是 usage 出口。

### 2.4 分母只留一个选择点

Server 侧**同时提供**两个值，不做选择：`context_window_tokens`（窗口）与 `max_input_tokens`（输入预算）。
选择发生在 Client 一处（`raw_cap`），并把它从注释升为 Client ADR 条目。

理由（已写在 Client `context_axis.rs` 头部注释里，此处只是固化）：窗口在会话内恒定、可预期；
若首轮前用窗口、首轮后切 `max_input_tokens`，分母会在 1000k / 993k 之间跳变。
**本设计不改这个选择**，只把「两个值都从 Server 拿」落到实处（今天 `/status` 已经给
`llm_context_tokens`，但没有预算推导结果，Client 得自己算 trigger/target）。

## 3. 实施步骤

### S3a：类型与单源（不改任何出口的数值）

1. `cm_types` 加 `TurnInputUsage` + `from_report`；`ContextTokenEstimate` / `ContextTokenBudget`
   目前定义在 `agent::context_compaction`，需要一个 `cm_types` 可见的镜像或上移（二选一，见 §6 Q1）。
2. `context_trim.detail`、SSE snapshot、`/status` 内部改为从 `TurnInputUsage` 取值，
   **字段名与数值一律不变**。
3. 单测：同一 `ContextCompactionReport` 经三出口得到的 `estimated.used_input_tokens` 逐位相等。

验收：`cargo test --workspace` 全绿；golden 全绿（S3a 不动压缩行为）。

### S3b：口径分离（破坏性，需 Client 同步）

1. SSE：`used_input_tokens` 只填 estimated，`provider_input_tokens` 独立；`prompt_tokens` 语义恢复。
2. `/status`：新增 `context_budget`，补契约测试。
3. doctor：文案改「非回合实测」。
4. 文档：`docs/SSE协议.md`、`docs/命令行与路由.md`（`/status` 字段）、`docs/配置说明.md`（如涉分母说明）。
5. Client：状态栏改为 `estimated` 口径；`prompt_tokens` 若有消费点，同步。

## 4. 破坏性影响与迁移

| 变更 | 影响 | 迁移 |
| --- | --- | --- |
| SSE `used_input_tokens` 不再可能是 provider 值 | 有 provider usage 的供应商上，数值会降 1~3%；provider 缺失时不变 | Client 同版本发布；`client-contract-v*` 钉子 |
| SSE `prompt_tokens` 从 message-only 恢复为总输入 | 若 Client 用它做分母/展示，数值会**升**约 29K | 先查 Client 三个消费点再改；必要时 S3b 拆两步 |
| `/status` 新增 `context_budget` | 纯新增，向后兼容 | 无 |
| doctor 文案 | 纯展示 | 无 |

`SSE_PROTOCOL_VERSION` **不需要** bump：本设计不改事件结构，只改某个已有字段的取值口径与新增可选字段。
但那意味着「字段名不变、语义变」——属于必须在 release note 里明写的类型，不能靠版本号兜。

## 5. 备选方案（未采纳）

1. **让 `context_trim` 也读 provider 值**：会让「服务端自己算的用量」变成「上游说了算」，
   而上游可能不回 usage（流式、部分网关），且回填发生在本次请求**之后**——`context_trim` 是在请求**之前**发的。
2. **在 `/status` 里存最近 usage**：见 §2.3。
3. **Client 侧自己拆两个数**：那需要 Client 也跑 tiktoken 或再解析一遍，等于把口径复制一份。
4. **改分母为输入预算**：会让所有百分比整体上移约 2%，且引入首轮前后分母跳变的旧问题。

## 6. 待拍板

1. `ContextTokenEstimate` / `ContextTokenBudget` 今天是 `agent::context_compaction` 里的 pub 类型。
   `TurnInputUsage` 放 `cm_types` 就要么在 `cm_types` 里镜像一份（两份定义，需同步测试），
   要么把两个类型上移到 `cm_types`（改 import 面，触及 `context_compaction` / `context_timeline`）。
   **建议上移**——它们是纯数据、无依赖，正是 `cm_types` 的定位。
2. `prompt_tokens` 语义恢复要不要拆成独立的第三个 PR（S3c）？它比 `used_input_tokens` 更危险，
   因为「变大了」不容易被注意到。
3. Client 分母策略的 ADR 编号与落点（Client 仓 ADR 0005 是否容纳，还是新开一条）。
