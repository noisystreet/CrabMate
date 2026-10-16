# ADR：上下文窗口压缩策略（Token 主导 + 跨回合基线化 + 原文归档）

**状态**：**Accepted** —— Phase 1–3（Token 主导压缩）已实施；Phase A（canonical 基线化 + 原文归档 + `derive` 简化）已实施；Phase B（旁路收口）已实施；Phase C（耦合面与配置）已实施：**C1（分支锚点 re-expand）/ C2（导出携带归档）/ C3（配置与文档）/ C4（`prune` 覆盖归档）全部落地**
**日期**：2026-08-24（初稿）；2026-10-15（合并两篇 ADR 并修订边界）；2026-10-16（Phase C 全部落地，状态改 Accepted）
**决策范围**：Server 上下文计量、模型请求视图派生、裁剪/压缩事件、会话持久化（`conversation_store`）、`ModelContextView` 派生、`run_agent_turn` 回合提交、旁路调用点（`builtin_btw` 等）；无 SSE 协议变更，HTTP 仅新增**向后兼容的软字段**（C2：`/conversation/messages` 可选 `include_archive` 查询参数 + 可选 `archive` 响应字段，默认不返回），旧 Client 无需改动
**关联文档**：

- [`context_trimming_scheme.md`](./context_trimming_scheme.md)：当前会话同步管道事实说明
- [`context_window_management_react_pruning.md`](./context_window_management_react_pruning.md)：ReAct 长循环裁剪早期设计稿
- [`../配置说明.md`](../配置说明.md)：上下文配置真源
- [`../SSE协议.md`](../SSE协议.md)：`context_trim` / `timeline_log` 协议

> 本文合并自原 `context_window_token_aware_compaction.md`（Token 主导压缩，Phase 1–3 已实施）与 `context_window_cross_turn_compaction.md`（跨回合压缩，Proposed）；两文已于 2026-10-15 合并入本文并删除。

---

## 1. Context

### 1.1 计量口径与消息条数（早期问题）

CrabMate 当前在每次模型调用前依次执行：

1. 压缩超长 `tool` 正文；
2. 按 `max_message_history` 删除旧消息；
3. 按近似字符预算删除旧消息；
4. 可选将中段历史摘要为一条消息。

做出本决策前，默认 `max_message_history = 24` 按**消息条数**计数。工具型 ReAct 的一次用户回合会产生：

```text
user
assistant(tool_calls)
tool
assistant(tool_calls)
tool
assistant(final)
```

因此少量用户回合就可能超过 24 条，尽管真实 Token 用量距离模型窗口仍很远。

当前还存在以下口径差异：

- 裁剪管道使用消息条数与 `bytes / 2` 字符近似；
- Client 底栏使用 tiktoken prompt 粗估；
- tiktoken 粗估不含工具 schema JSON 与图片 Token；
- Client 用完整 `llm_context_tokens` 作分母，但模型输入还需预留输出空间；
- `context_trim` 同时表示历史删除、LLM 摘要与单条工具输出压缩；
- 仅 `compress_hits > 0` 时也显示“已裁剪历史”，容易被理解为历史消息已丢失。

这些差异使“显示用了多少”与“为什么开始裁剪”无法直接对应。

### 1.2 跨回合压缩失效现象

从一份真实导出会话（长会话中反复读取同一个大文件）的压缩记录观察到：

| 模型调用 | `before_tokens` | `after_tokens` | `compaction_reason` |
|---|---|---|---|
| … | 1,108,621 | 731,317 | `token_budget_summary` |
| … | 1,108,823 | 388,512 | `token_budget_summary` |
| … | 1,280,495 | 216,914 | `token_budget_summary` |

两个反直觉现象：

1. **水位（`after_tokens`）持续下降，却仍然每若干回合必触发压缩**——压缩仿佛“白做”了；
2. **`before_tokens` 逐次抬升**——压缩前的“输入”不但没被压下去，反而越来越大。

### 1.3 机制

沿调用链核实后，当前实现如下（Phase 3 的产物）：

1. 每个回合开始时，[`RunAgentTurn`](file:///home/gzz/crabmate/server/src/run_agent_turn.rs#L173-L174) 调用 `ModelContextView::derive(messages)`，把**规范会话历史 `canonical` 全量克隆**为本次模型视图。
2. 同步压缩、LLM 摘要、旧工具输出压缩**只修改该视图副本**（`messages_buf = model_context_view.messages_mut()`）。
3. 回合成功后 [`commit_successful_model_context_view`](file:///home/gzz/crabmate/server/src/run_agent_turn.rs#L296-L310) 只做两件事：
   - 把 `ModelContextArtifact` 序列化为隐藏 system 旁注 **push 进 `canonical`**；
   - 调 [`commit_current_turn_to`](file:///home/gzz/crabmate/server/src/agent/model_context_view.rs#L201-L230) 把**当前真实用户回合之后新增的消息追加回 `canonical`**。
4. `canonical` 被再次写回 SQLite（同一 `revision` 原子保存），但**从不删减**。

而 [`ModelContextView::derive`](file:///home/gzz/crabmate/server/src/agent/model_context_view.rs#L169-L182) 当前实现是：

```rust
pub fn derive(canonical: &[Message]) -> Self {
    Self {
        messages: canonical.to_vec(),                       // 全量克隆
        canonical_message_count_before_turn: canonical.len(),
        original_parked_sidecars: canonical.iter()          // artifact / timeline 旁注原样带入
            .filter(|m| is_chat_timeline_marker(m) || is_model_context_artifact_marker(m))
            .cloned().collect(),
    }
}
```

它**不消费**历史 `ModelContextArtifact`——旁注只是被原样带进视图，artifact 里记录的“上次摘要到哪、移出了哪些组”没有任何回归路径去应用。

### 1.4 根因

| 编号 | 根因 | 证据 |
|---|---|---|
| **R1** | `canonical` 只增不减 | `commit_current_turn_to` 只有 `canonical.extend(...)`，无删减；`conversation_store::prune` 只按**会话条数**与 TTL 淘汰，**没有单会话消息条数上限** |
| **R2** | `derive` 不应用既有 artifact，跨回合不生效 | `derive` 仅 `to_vec()`；`ModelContextArtifact` 无任何生产路径消费者（`replay_removed_messages` 仅测试使用） |
| **R3** | 旁路未覆盖 | `builtin_btw` 走 [`build_messages_for_turn`](file:///home/gzz/crabmate/server/src/web/chat_handlers/chat/turn_build.rs#L351-L387) → `build_existing_conversation_turn`，仅 `seed.messages.push(last_user)` 后直接发供应商；既不经 `derive` 也不经 `prepare_messages_for_model`，**全程零压缩** |

三者叠加的后果：**每回合都从“满血历史”重新算一遍，压缩成果每回合被丢弃再重做**。`after_tokens` 的下降只是“这一次调用”的瞬时值，不影响下一回合的起点；而每次摘要在 `canonical` 中留下一条 artifact 旁注，进一步推高 `before_tokens`。于是“越压越大、每轮必压”。

### 1.5 与“不得永久删除历史”原则的关系

既有设计原则曾声明：

> 裁剪不得**永久删除**规范会话历史。摘要可作为派生工件持久化，但不能替代原始消息。

跨回合基线化要动 `canonical` 的形态（把被摘要覆盖 / 被移出组的原文从 `canonical` 移走），与该条存在**表面冲突**。处理方式不是推翻它，而是**收紧它的边界**：

- 该原则的内核是“**原始消息不得被静默丢弃、必须仍可审计与回放**”，而不是“原始消息必须永远留在同一个 `Vec` 里”；
- 本 ADR 通过 **`crabmate_conversation_archive` 原表原样保留被移出的消息**（含 `reason` 与区间），实现“可审计 + 可回放”，从而满足该原则的**内核**；
- 该条中“摘要不能**替代**原始消息”仍然成立：摘要是**模型可见基线**的组成部分，归档才是原文归属地；二者并存，摘要不销毁原文。

修订后的原则正文见 §3.4。

---

## 2. Decision Drivers

按优先级排序：

1. 模型请求必须稳定落在真实输入窗口内；
2. **压缩成果必须跨回合稳定保持**——否则成本、延迟、水位抖动都无法收敛；
3. 不得破坏 `assistant.tool_calls` 与 `tool` 的结构关系；
4. **每回合不得全量克隆 / 全量重算封顶之外的历史**；
5. `canonical`（及其 artifact 旁注）必须封顶，否则内存、落盘与导出体积线性膨胀；
6. **可审计原文不得丢失**，且回放能力应可产品化（而非仅测试）；
7. 持久化会话应可审计，不应等同于压缩后的模型视图；
8. 既有语义不得破坏：分支截断、`/conversation/messages` 导出、`/uploads/` 引用扫描、revision 原子性；
9. **旁路必须收口**到同一压缩入口，避免“某条路径偷偷不压”；
10. Server 与 Client 应使用可解释、尽量一致的预算口径；
11. 少量工具型回合不应仅因消息条数较多而提前删除历史；
12. 改造须分阶段、可回滚，兼容现有 SSE、SQLite 会话与旧 Client（无协议变更）；
13. 无供应商精确计数能力时仍须有保守、确定性的降级路径。

---

## 3. Decision

### 第一部分：Token 主导的压缩（Phase 1–3，已实施）

#### 3.1 Token 预算成为主要触发条件

模型调用前计算：

```text
max_input_tokens
  = context_window_tokens
  - reserved_output_tokens
  - safety_margin_tokens

estimated_input_tokens
  = system_and_messages_tokens
  + tool_schema_tokens
  + attachment_tokens
  + vendor_overhead_tokens
```

规则：

- `reserved_output_tokens` 默认采用本次请求的 `max_tokens`；
- `safety_margin_tokens` 必须非零，吸收供应商分词和消息封装误差；
- 已知模型优先使用匹配 tokenizer；
- 供应商返回可靠 usage 时，用它校准后续显示与估算；
- 未知模型回退到保守字符估算，默认按不高于 3 chars/token 估算；
- `max_message_history` 降级为异常安全上限，不再作为日常主要裁剪条件。

触发阈值与目标值配置化：

- 达到安全输入预算的约 85% 时开始压缩；
- 压缩目标为安全输入预算的约 65%～70%；
- 具体默认值须通过真实 Agent benchmark 与回归夹具校准，不写死在协议中。

#### 3.2 按完整交互组裁剪

定义 `ConversationTurnGroup`，最小逻辑单元为：

```text
user
  + assistant(tool_calls) / tool 的完整链
  + assistant(final，可选)
```

裁剪与保留必须以完整组为单位：

- 不允许保留孤立 `tool`；
- 不允许删除 `assistant(tool_calls)` 后仍保留其结果；
- 当前用户输入与正在执行的工具链不可裁；
- 最近若干完整组优先保留；
- system、已确认约束、审批结论及关键失败证据具有更高保留优先级。

现有 `drop_orphan_tool_messages` 保留为防御性兜底，不作为正常裁剪后的修复手段。

#### 3.3 分级压缩

按以下顺序释放预算：

1. 压缩旧工具输出，保留摘要、错误码、路径、退出码与截断标记；
2. 删除不需回传模型的旧 reasoning 正文；
3. 将较早的完整交互组摘要为结构化状态；
4. 删除已被摘要覆盖的最旧完整交互组；
5. 极端情况下进入降级视图，仅保留任务、约束、近期交互与必要证据。

结构化摘要至少覆盖：

- 当前目标与验收条件；
- 已完成事项；
- 失败尝试与错误原因；
- 关键文件、符号和决策；
- 未完成步骤与下一动作。

#### 3.4 持久化历史与模型视图分离

长期目标是：

- `ConversationHistory`：SQLite 中的完整、可审计历史；
- `ModelContextView`：每次模型调用前派生的压缩视图；
- `ContextCompactionReport`：描述本次视图如何生成。

**原则（2026-10-15 边界修订后）**：

> 裁剪不得丢失规范会话原始消息；原始消息必须保留在可审计存储中（规范历史**或归档表**），可按区间回放。摘要可作为模型可见基线持久化并参与后续派生，但**不得**成为原始消息的唯一副本。

即：原文可迁入 `crabmate_conversation_archive` 归档（见 §3.8），而不再要求其始终留在规范历史 `Vec` 内；原则内核（可审计 + 可回放）不变。本 ADR 合并前曾表述为“裁剪不得永久删除规范会话历史”，本节取代之。

在完成分离前，现有就地 `Vec<Message>` 管道继续作为兼容实现；新增逻辑不得进一步扩大其职责。

#### 3.5 计量与展示口径

Server 是上下文用量真源。Client 不自行推断是否发生裁剪。

最终应向 Client 提供：

- `used_input_tokens`；
- `max_input_tokens`；
- `reserved_output_tokens`；
- `message_tokens`；
- `tool_schema_tokens`；
- `attachment_tokens`；
- `counting_source`：供应商 usage、匹配 tokenizer 或保守估算；
- `before_tokens` / `after_tokens`；
- `compaction_reason`。

Client 进度条分母使用 `max_input_tokens`，不再使用未扣除输出预留与安全余量的完整上下文窗口。

#### 3.6 事件语义拆分

以下行为不得继续共用“已裁剪历史”文案：

- `history_compacted`：删除历史组或生成历史摘要；
- `tool_output_compressed`：仅压缩工具输出；
- `context_budget_warning`：接近预算但尚未压缩。

兼容策略：

- 第一阶段不新增 SSE 顶层键、不提升 `SSE_PROTOCOL_VERSION`；
- 继续使用 `timeline_log.kind = "context_trim"`；
- 仅工具压缩时标题改为“已压缩工具输出”；
- 发生条数/字符裁剪或摘要时标题保持“已裁剪历史”；
- 后续可在 `detail` 增加可选 `reason` / Token 字段，旧 Client 可安全忽略；
- 若未来新增顶层控制键，须执行 Server/Client 协议金样与版本清单。

### 第二部分：跨回合压缩——canonical 基线化 + 原文归档（Phase A / B 已实施；Phase C 待实施）

四项决策，合起来构成“彻底方案”。

#### 3.7 canonical 基线化（Baseline Commit）——消除 R1

回合成功提交从“**只追加**”升级为“**对账（reconcile）**”，且**只提交结构差异**：

```text
canonical_after := canonical 去掉「已被摘要 / 被移出的前缀原文」
                  + 1 条基线摘要（替代该前缀）
                  + 保留窗口内其后原文「逐字不变」（不写入本回合视图里的内容改写）
                  + 当前回合新增的真实 assistant/tool/时间线
被移出的前缀原文区间 → 写入归档表（携带 reason / 区间 / seq）
```

要点：

- 摘要**成为 canonical 的一部分**：canonical 内以 `CRABMATE_CONTEXT_SUMMARY_NAME` 注入形状保留一条“基线摘要”，替代被它覆盖的前缀原文；
- `canonical` 因此**不再单调增长**：压缩一旦发生，`canonical.len()` 随之下降并封顶；
- **canonical 只做“结构性删减 + 摘要插入”，不做“就地内容改写”**：模型视图里对本就在保留窗口内的消息所做的**工具正文压缩**（[`compress_tool_message_contents`](file:///home/gzz/crabmate/server/src/cm_agent/message_pipeline/transforms.rs#L103-L122)，`*s = compressed`）与 **reasoning 剥离**属**视图层优化**，**不得**回写 canonical，canonical 内这些消息保持原文。否则“被就地改写的原文”既不在任何“移出区间”内、也不会进归档，会**绕过归档被静默丢失**；
- 由此，工具正文压缩会在每个回合对**有界的保留窗口**重新计算一次——它是确定性的、成本有界的，不构成“每回合全量重算”；
- “被移出组的原文”与“被摘要覆盖的原文”**不丢弃**，原样迁入归档（见 §3.8）。

##### 3.7.1 提交前的净化链（必须显式复用，不得依赖隐式副作用）

虽然 reconcile 只提交结构差异，但本回合**新增**的 assistant / tool / 时间线消息取自模型视图，仍可能连带把本回合的**临时注入**一起固化进 `canonical`。因此提交前必须经过与落盘**完全相同**的剥离链（抽为单一命名函数 [`strip_persist_injections`](file:///home/gzz/crabmate/server/src/chat_job_queue/stream_finish.rs#L44-L49)，由 [`post_turn_web_prepare_and_save`](file:///home/gzz/crabmate/server/src/chat_job_queue/stream_finish.rs#L77) 调用），并写死为契约：

- [`strip_long_term_memory_injections`](file:///home/gzz/crabmate/server/src/cm_memory/memory/long_term_memory.rs#L730) — 长期记忆 `CRABMATE_LONG_TERM_MEMORY_NAME`；
- [`strip_workspace_changelist_injections`](file:///home/gzz/crabmate/server/src/cm_tools/workspace/changelist.rs#L213) — 变更集 `CRABMATE_WORKSPACE_CHANGELIST_NAME`；
- [`strip_orchestration_injected_users_for_conversation_store`](file:///home/gzz/crabmate/server/src/cm_types/server_injected_user.rs#L37-L47) — 其余编排注入 user；
- `is_execution_constraint_ephemeral_system` 的 **system** 注入已由 [`outer_loop` 在每次模型调用后 retain 移出](file:///home/gzz/crabmate/server/src/agent/agent_turn/loop/outer_loop.rs#L403-L404)，正常情况下不进入提交；该步已并入 `strip_persist_injections`（**行为变更**：残留的执行约束 system 现于落盘前被显式剔除，不再依赖 `outer_loop` 的隐式副作用；原先此类残留会被固化进 `canonical`）。

**基线摘要的优先级（关键）**：`CRABMATE_CONTEXT_SUMMARY_NAME` 恰在上述 strip 的**保留名单**内（与首轮工作区画像 `CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME` 一致），因此“摘要进 canonical”与“注入剥离”并不冲突——剥离链**保留摘要、剔除其余注入**。此优先级必须在实现中显式声明，不得依赖调用顺序的偶然性。

#### 3.8 原文归档表（Archive）——保留可审计性

新增归档存储（新表，见 §4），每条记录一个被移出的连续区间：

```text
(conversation_id, seq, reason, range_start, range_end,
 messages_json, codec, created_at_unix)
```

- **归档恒为“被移出的原文前缀”**：压缩始终作用于最旧一端——`remove_oldest_turn_group` 删最旧组；LLM 摘要保留 `system` 与尾部 `context_summary_tail_messages` 条、替换其前的**全部**原文（见 [`apply_llm_summary_to_messages`](file:///home/gzz/crabmate/server/src/agent/context_window.rs#L378-L391)）。因此“归档 ∪ canonical 原文部分 = 完整原始历史”，且归档按 `seq` 升序即为原始先后顺序。**这是 re-expand（§3.11）成立的前提，不得破坏**（例如未来若引入“中段摘要”，必须同步改造 re-expand）；
- `reason` ∈ { `summarized`（被摘要覆盖）, `removed_turn_group`（按 Token 预算移出）, `artifact`（被回收的旧 artifact 旁注，见 §3.9）};
- `seq` 在会话内单调递增；
- `range_start` / `range_end` 是**写入时刻 canonical 的下标快照**，仅供人审与排查；re-baseline 后**不再对应任何现存 canonical**，**不得**用作回放 / re-expand 的定位依据（回放按 `seq` 顺序 + `messages_json` 内容重放）；
- `messages_json` 允许压缩存储：`codec ∈ { identity, gzip }`，复用仓库已有 `flate2`（**不引入 zstd**——`Cargo.toml` 明确回避 `zstd-sys` 的 C 编译负担）。**选择策略**：单个区间的 `messages_json` 原始字节数 ≥ `context_archive_codec_min_bytes`（新配置，默认如 4 KiB）时写 `gzip`，否则写 `identity`；解压失败时**不得静默丢弃**，应回退为可诊断错误。`codec` 与 `messages_json` 同事务写入；
- **`list_all_messages_json` 必须覆盖归档表**（`/uploads/` 引用扫描），否则被归档消息引用的上传文件会被误判为孤儿并被清理。该收口必须与归档写入**同一 PR（Phase A）**完成——这是必须同步的**隐藏耦合**（见 §4.2 / §5）；
- 归档按会话设**保留上限**（新配置 `context_archive_max_rows_per_conversation`），并与 `prune` 的 TTL 联动回收。**注意**：回收会破坏“完整 re-expand”，因此该上限应显著大于常规压缩产生的区间数；一旦某分支点早于保留窗口，**re-expand 不可完成**，分支接口必须返回**显式错误**（而不是静默截断到可见范围）。故措辞以“**在保留窗口内可回放**”为准，不再宣称“永久可查”。

#### 3.9 `derive` 退化为“读已基线化的 canonical”——消除 R2

基线化落地后，`canonical` **本身就是模型视图**（已含摘要、已移除旧组）。因此：

- `derive` 不再需要“重放 artifact 来构造视图”——直接 `canonical.to_vec()` 即为正确视图；
- `ModelContextArtifact` **语义降级为审计凭证 + 归档指针**：记录“这次调用相对上一条基线压了什么”，供导出与人类排查，而不再承担“跨回合恢复视图”的职责；
- **artifact 旁注的回收路径（必须明确，否则 §9.2 验收项 3 无法验收）**：`canonical` 内**至多保留最近一条** artifact 旁注；被替换的旧旁注以 `reason = 'artifact'` 写入归档表（见 §3.8），而不是继续在 `canonical` 中累积。`GET /conversation/messages.context_artifacts` 仍从 `canonical` 解析，**数组形状不变**（仅条数由“无界累积”变为“至多 1 条”）；
- 由于 §3.7 已让 `canonical` 封顶，`to_vec()` 的克隆规模随之有界（R2 的“每回合全量重算”被消除）。

> 说明：这一步**不引入**“derive 每次读 artifact 做删减”的复杂路径。基线化把复杂度前置到“回合提交”这一处，派生保持简单，符合 §3.4 “持久化历史与模型视图分离”的定位。

#### 3.10 旁路收口——消除 R3

`builtin_btw` 等旁路必须走**同一套**“构建模型请求视图”逻辑，但保持其“**不落盘**”语义（btw 是旁路查询，不得改写 canonical）。做法：

- 抽出只读入口：输入 `canonical` + 本轮 user，输出“发模型的 messages”，内部复用 `prepare_messages_for_model` 的**同步压缩路径**，但**不写回 canonical、不写归档、不写 artifact**；
- **btw 默认不触发 LLM 摘要**：只做同步裁剪 + 旧工具输出压缩。btw 是交互式旁路（[`try_acquire_owned` 队列满立即失败](file:///home/gzz/crabmate/server/src/web/chat_handlers/chat/builtin_btw.rs#L119-L122)），长会话下额外摘要调用会抬高延迟与费用；确需摘要时通过显式配置开关 `context_btw_summary_enabled`（默认 `false`）启用——开启后只读入口按与主路径**相同**的触发规则尝试一次摘要，但仍**不**写 canonical / 归档 / artifact；
- 主路径（`run_agent_turn`）用同一入口的“可提交”变体；
- 通过类型/参数区分“dry-run 视图”与“可提交视图”，避免未来再出现“忘记压缩”的新旁路。

#### 3.11 分支锚点与 re-expand——修复“基线化破坏分支截断”

基线化后 canonical 只保留“基线摘要 + 最近若干组”，更早的真实用户回合的原文只存在于归档；而分支接口（`POST /chat/branch`）当前按 **canonical 内的真实 user 序号**定位截断点（[`branch_truncation_cut_index`](file:///home/gzz/crabmate/server/src/web/app_state.rs#L455-L466) / [`truncate_before_user_ordinal_if_revision`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L435-L466)）。**若沿用该口径，基线化会改变序号、并让“分支到已被归档的早期回合”无法定位**（压缩删除的是含真实 user 的最旧完整组，见 [`conversation_turn_groups`](file:///home/gzz/crabmate/server/src/cm_agent/message_pipeline/transforms.rs#L21-L37) / [`remove_oldest_turn_group`](file:///home/gzz/crabmate/server/src/cm_agent/message_pipeline/transforms.rs#L42-L45)）。因此必须显式定义锚点与回滚路径：

- **锚点：会话级全局序号 `user_turn_ordinal`**（0-based，**按真实用户回合计数、含已归档回合、跨归档稳定**），取代“canonical 内下标”。注入条（长期记忆 / 变更集 / 首轮画像 / 上下文摘要）不计入，沿用 [`user_message_counts_for_branch_truncation`](file:///home/gzz/crabmate/server/src/cm_types/message.rs#L300-L333) 的判定；
- **re-expand（回灌）**：分支点落在归档前缀时，先按 `seq` 升序把归档区间的 `messages_json`（必要时先 `gzip` 解压）重放回 canonical，得到**完整原文历史**，再按 `user_turn_ordinal` 截断。re-expand 后的 canonical 不再含基线摘要（摘要所代表的原文已还原）；
- **归档处置**：re-expand 完成后，该会话的旧归档行**全部失效**（其内容已回到 canonical，或属于被丢弃的未来），应在**同一事务**内清除；后续回合重新压缩时再生成新的归档行。**这不是“按水位删前缀”**——回滚时不删除仍需保留的原文，只清除“已被重放消费 / 属于被丢弃未来”的行；
- **一致性**：`canonical 更新 + 归档清除` 必须在**同一 revision 的同一事务**内完成，避免半提交；
- **可判定性**：判断分支点是否落在归档前缀，只需比较“归档中的真实用户回合数”与目标序号（归档恒为前缀，见 §3.8），**无需额外水位列**。

> 说明：本节**取代**合并前 ADR 中“归档按 `archived_through_seq` 水位随分支一并裁剪（删除）”的表述——该水位方向相反（会删掉仍需保留的前缀原文），且未定义“分支点 → 水位”的换算，故一并移除该列（见 §4.1）。

---

## 4. 数据 / 存储设计

### 4.1 表结构

在 `conversation_store` 所在 SQLite（`crabmate_conversations` 同库）新增：

```sql
CREATE TABLE IF NOT EXISTS crabmate_conversation_archive (
    conversation_id  TEXT    NOT NULL,
    seq              INTEGER NOT NULL,
    reason           TEXT    NOT NULL,
    range_start      INTEGER NOT NULL,
    range_end        INTEGER NOT NULL,
    messages_json    TEXT    NOT NULL,
    codec            TEXT    NOT NULL DEFAULT 'identity', -- identity | gzip
    created_at_unix  INTEGER NOT NULL,
    PRIMARY KEY (conversation_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_crabmate_conversation_archive_conv
    ON crabmate_conversation_archive(conversation_id);
```

读取按 `WHERE conversation_id = ? ORDER BY seq` 即可支撑 re-expand（§3.11）；会话行**不新增水位列**（原 `archived_through_seq` 已在 §3.11 移除）。迁移沿用 [`migrate`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L28-L44) 的幂等风格；`messages_json` 复用 `messages_to_json` / `messages_from_json` 形状，不新增格式版本。

### 4.2 与既有语义的耦合点

**阶段标注**：`list_all_messages_json` 与 `save_if_revision` 原子性**必须与归档写入同 PR（Phase A）**完成，否则 Phase A 一开始写归档、canonical 收缩，而扫描仍只读 canonical，就会产生“被归档消息引用的 `/uploads/` 文件被判孤儿 → 物理删除”的**不可逆窗口**（原表头“必须同 PR 处理”与 §5 排期自相矛盾，此处按严重度重新分级）。

| 既有函数 / 接口 | 影响 | 处理 | 阶段 |
|---|---|---|---|
| [`list_all_messages_json`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L333) | 供 `/uploads/` 引用扫描；不覆盖归档会误删仍在引用的上传文件 | **必须**合并归档表内容 | **Phase A（已实施）**（与归档写入同 PR） |
| `save_if_revision` / revision 原子性 | canonical + 归档 + artifact 旁注必须**同 revision 原子写** | 在同一事务内完成，避免半提交（会话行不新增水位列，见 §4.1） | **Phase A（已实施）** |
| `GET /conversation/messages.context_artifacts` | 契约面：`artifacts_from_messages` 从 canonical 解析旁注 | 数组形状不变；canonical 至多保留最近一条 artifact（见 §3.9） | **Phase A（已实施）** |
| [`truncate_before_user_ordinal_if_revision`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L435-L466) | 现按 `user_message_counts_for_branch_truncation` 统计 **canonical 内**真实用户序号后 `messages.truncate(cut)`；**基线化后 canonical 内序号会偏移，且早期回合原文已不在 canonical，直接沿用会截错或无法定位** | 分支锚点改用**会话级 `user_turn_ordinal`（含归档，见 §3.11）**；分支点落在归档前缀时先 **re-expand（§3.11）** 还原完整原文再截断；`canonical 更新 + 归档清除` 同事务；补“分支到已归档早期回合”用例 | **Phase C（C1 已实施）** |
| [`prune`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L300) | 按会话条数 / TTL 淘汰；删除会话时归档行也必须删；超 `context_archive_max_rows_per_conversation` 的归档行需回收 | 同事务 `DELETE ... WHERE conversation_id = ?`；回收会使**早于保留窗口的分支点无法 re-expand**，须由分支接口返回显式错误（见 §3.8 / §3.11） | **Phase C（C4 已实施）** |

### 4.3 封顶效果

设基线摘要 1 条、保留最近 `context_summary_tail_messages`（默认 12，clamp 4–64）个完整交互组、`MIN_RECENT_TURN_GROUPS = 2`：

```text
canonical.len() ≲ 1(system) + 1(基线摘要) + 1(artifact 旁注, 见 §3.9) + N(最近交互组) + 当前回合
```

`N` 有界 ⇒ `canonical` 长度有界 ⇒ 每回合克隆与重算成本有界。可另加 `max_message_history`（Phase 2 已提至 256）作为异常安全兜底。

---

## 5. Delivery Plan

### 5.1 已实施：Phase 1–3

#### Phase 1：缓解提前裁剪并修正文案（已实施）

范围：

1. 默认 `max_message_history` 从 24 调整为 64；
2. 保留环境变量与 TOML 覆盖能力；
3. 仅 `compress_hits > 0` 时显示“已压缩工具输出”；
4. 历史确实被删除或摘要时才显示“已裁剪历史”；
5. Client 设置文案由“注入与窗口裁剪”调整为“上下文注入与压缩”；
6. 增加回归测试，覆盖工具压缩与历史裁剪标题分流；
7. 同步配置文档、SSE 文档和 CHANGELOG。

本阶段不改变裁剪算法与 JSON 形状，可独立回滚。

#### Phase 2：Token 主导与交互组完整性（已实施）

范围：

1. 建立最终请求 Token 预算器；
2. 计入 tools JSON、system、消息和附件预算；
3. 实现 `ConversationTurnGroup`；
4. 以 Token 阈值触发压缩；
5. 将 `max_message_history` 提至 256，变为高位安全兜底；
6. 输出 `ContextCompactionReport`；
7. 用工具密集型 fixture 验证 2～4 个用户回合不会无故裁剪。

#### Phase 3：历史与模型视图分离（已实施）

范围：

1. `run_agent_turn` 从规范消息克隆派生 `ModelContextView`；同步裁剪、摘要与工具输出压缩只改派生视图，成功后仅把当前真实用户回合的新 assistant/tool/时间线写回，因而 SQLite 不再被模型窗口裁剪覆盖；
2. 每次实际模型调用前记录 `ModelContextArtifact`，以隐藏 `system.name=crabmate_model_context_artifact` 旁注持久化压缩报告、摘要、规范历史计数及被移出组/摘要区间；`GET /conversation/messages.context_artifacts` 返回这些软字段配方，原片段仍在完整规范历史中，可按区间回放；
3. `tiktoken_prompt_tokens` 保留原有 `prompt_tokens` / `tiktoken_model`，并新增可选 `used_input_tokens`、`max_input_tokens`、输出预留与消息/tools/附件分项；Client 优先用 Server 的 `used_input_tokens / max_input_tokens` 绘制底栏；
4. OpenAI 兼容 `usage.prompt_tokens` / `completion_tokens` 与 `input_tokens` / `output_tokens` 均可解析；上游返回输入用量时写入 `provider_input_tokens`，`counting_source=provider_usage`，供本轮最终快照校准。未返回时保留 tokenizer/保守估算来源；
5. 所有新增 HTTP/SSE 字段均为可选软字段；旧 Client 继续读取原有两字段，新 Client 对旧 Server 回退本地配置窗口。

### 5.2 跨回合压缩（Phase A–B 已实施；Phase C 待实施）

分三阶段，每阶段可独立回滚。

#### Phase A：基线化 + 归档 + derive 简化（含 `list_all_messages_json` 收口）（已实施）

1. 新增归档表（含 `codec`）与读写 / 裁剪 / re-expand 读取 API（会话行**不新增水位列**，见 §4.1 / §3.11）；
2. **`list_all_messages_json` 合并归档表内容**（与归档写入同一 PR，堵住孤儿文件不可逆清理窗口——见 §4.2）;
3. 把 `commit_successful_model_context_view` 升级为 reconcile 版本：**只提交结构差异**（前缀原文移出 → 归档；插入 1 条基线摘要；保留窗口内其后原文**逐字不变**），工具正文压缩 / reasoning 剥离等**就地内容改写仅停留在视图层、不回写 canonical**（见 §3.7）；提交前显式走净化链（见 §3.7.1）；
4. `ModelContextArtifact` 保留落盘（审计 / 导出）；canonical 至多保留最近一条旁注，旧旁注以 `reason = 'artifact'` 写入归档；`derive` 简化为读 canonical；
5. 回归：`view_compaction_does_not_delete_canonical_history` 等既有测试改为断言“原文在归档中仍可回放”；新增“被归档消息引用的 `/uploads/` 文件不被判为孤儿”用例。

**Phase A 落地状态与遗留**（2026-10-16 复核）：

- 已落地：归档表与读写 API（[`archive.rs`](file:///home/gzz/crabmate/server/src/conversation_store/archive.rs)）；结构性提交 reconcile（[`commit_current_turn_to`](file:///home/gzz/crabmate/server/src/agent/model_context_view.rs#L202-L277)）；同事务原子落盘（[`save_with_archive_if_revision`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L322-L346)）；`list_all_messages_json` 合并归档（A4）；`derive` 直读 canonical（A5）。
- Phase A 的临时口径：分支点落回归档前缀时**返回 `Conflict`**（不作 re-expand），由 Phase C1 替换为 re-expand + 超窗显式错误；归档 `codec` 阈值仍为常量 [`ARCHIVE_CODEC_MIN_BYTES_DEFAULT`](file:///home/gzz/crabmate/server/src/conversation_store/archive.rs#L83)（配置化见 C3）。
- 待接线的已实现 API：[`delete_archive_for_conversation`](file:///home/gzz/crabmate/server/src/conversation_store/archive.rs#L277) / [`prune_archive`](file:///home/gzz/crabmate/server/src/conversation_store/archive.rs#L292)（Phase C4 接线）。

#### Phase B：旁路收口（已实施）

1. 抽出“只读模型请求视图构建”入口（dry-run 压缩，不落盘；**默认不触发 LLM 摘要**，见 §3.10）；
2. `builtin_btw` 改走该入口；
3. 回归：长会话 btw 的 `before_tokens` 不再等于“全量原文”，且 btw **不写** canonical / 归档。

**Phase B 落地状态**（2026-10-16）：

- 新增只读入口 [`prepare_readonly_model_messages`](file:///home/gzz/crabmate/server/src/agent/context_window.rs#L626-L685)（**async**）：复用同步裁剪 + token 预算压缩，无 store 参数（结构性保证不落盘）；**默认不触发 LLM 摘要**，仅当传入 `Some(ReadonlySummaryLlm)`（由 `context_btw_summary_enabled=true` 提供）时才按主路径**相同**的触发规则尝试一次摘要。
- 共用助手 [`sync_trim_and_estimate_before`](file:///home/gzz/crabmate/server/src/agent/context_window.rs#L243-L261) / [`finalize_token_budget_compaction`](file:///home/gzz/crabmate/server/src/agent/context_window.rs#L264-L284) 由主路径与只读入口共用。
- [`answer_btw`](file:///home/gzz/crabmate/server/src/web/chat_handlers/chat/builtin_btw.rs#L157-L173) 在组装后调用该入口（`tools = &[]`，与 `no_tools_chat_request` 一致）；`summary_llm` 由 `context_btw_summary_enabled` 门控（默认 `false` → 不传）。
- 回归：`readonly_entry_bounds_tokens_without_summary_or_persistence`、`btw_readonly_summary_flag_defaults_off`（`agent::context_window::tests`）。

#### Phase C：耦合面与配置（C1 / C2 / C3 / C4 全部已实施）

1. **（已实施，C1）** 分支路径改造：`truncate_before_user_ordinal_if_revision` 改用**会话级 `user_turn_ordinal`**（含归档）并接入 **re-expand**；`prune` 覆盖归档表并补测（含分支点落回归档前缀时先 re-expand 再截断、回滚后同事务清除该会话归档；见 §3.11）；
2. **（已实施，C2）** 导出（`/conversation/messages`）支持可选携带归档（默认不返回，保持体积与兼容）：新增可选查询参数 `include_archive` 与可选响应字段 `archive`（`skip_serializing_if = "Vec::is_empty"`）；
3. **（已实施，C3）** 新增 / 复用配置项并同步 `配置说明.md`：归档保留上限 `context_archive_max_rows_per_conversation`（默认 256，`0` = 不限）、归档 codec 选择阈值 `context_archive_codec_min_bytes`（默认 4096，见 §3.8），以及旁路摘要开关 `context_btw_summary_enabled`（默认 `false`，见 §3.10）；`context_baseline_*` **已确认不再需要（废弃）**——基线摘要复用既有 `context_summary_*` 配置，不引入独立基线键；
4. **（已完成）** 更新 `CHANGELOG.md`；C2 完成后本文状态已改为 **Accepted**（见状态头）。

### 5.3 实施拆解（§5.2 工程附录）

> 本节是 §5.2 的**文件级落地拆分**（顺序 / 落点 / 验证 / 风险），仅细化实施，不改变 §3–§4 的契约；如有冲突以正文为准。顺序 **A → B → C**，每阶段可独立回滚；**A0 / A1 必须先于 A2**。

#### 5.3.1 目标与不变式

- **R1**（canonical 只增不减）→ reconcile **结构性提交**（§3.7）；
- **R2**（`derive` 不应用既有 artifact）→ canonical 基线化后 `derive` 退化为直读（§3.9）；
- **R3**（`builtin_btw` 零压缩）→ 旁路收口（§3.10）；
- 必须由测试守护的不变式：**「归档 ∪ canonical 原文 = 完整原始历史」**；归档恒为**被移出的原文前缀**（§3.8）；分支锚点为**会话级 `user_turn_ordinal`（含归档）**（§3.11）。

#### 5.3.2 Phase A 工程拆解（已实施）

**A0 — 归档表与迁移**：在 [`migrate()`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L28-L45) 新增 `crabmate_conversation_archive`（列见 §4.1，PK `(conversation_id, seq)`）+ 会话索引；幂等迁移，**不新增会话行水位列**；`codec ∈ { identity, gzip }`，`gzip` 走已有 `flate2`。

**A1 — 归档读写 / 回收 API**：在 `conversation_store.rs` 新增：
- `append_archive_rows`（同事务批量插入，`seq` 单调递增，按 `context_archive_codec_min_bytes` 阈值选 `codec`，默认 4 KiB）；
- `load_archive_messages(conn, id) -> Vec<Message>`（`ORDER BY seq`，gzip 解压；解压失败返回可诊断错误、**不静默丢弃**）；
- `delete_archive_for_conversation`、`prune_archive`（保留上限 `context_archive_max_rows_per_conversation` + TTL）。

**A2 — 结构性提交（核心重构）**：现状 [`commit_successful_model_context_view`](file:///home/gzz/crabmate/server/src/run_agent_turn.rs#L296-L310) 只 push artifact + 追加当前回合，[`commit_current_turn_to`](file:///home/gzz/crabmate/server/src/agent/model_context_view.rs#L201-L230) 只 append（canonical 永不收缩）。改造：
1. **捕获被移出前缀**：把 [`remove_oldest_turn_group`](file:///home/gzz/crabmate/server/src/cm_agent/message_pipeline/transforms.rs#L42-L51) 由「返回条数」改为**返回被移除的 `Vec<Message>`**；在 [`apply_llm_summary_to_messages`](file:///home/gzz/crabmate/server/src/agent/context_window.rs#L378-L391) `truncate(1)` 前捕获前缀原文；用 `StructuralCommit { archived_prefix, reason, baseline_summary }` 在 [`prepare_messages_for_model`](file:///home/gzz/crabmate/server/src/agent/context_window.rs#L477-L551) 链路上**累加**（同步裁剪 + token 预算压缩 + LLM 摘要多处均产生前缀）。
2. **提交规则**（§3.7）：`canonical_after = [system] + [基线摘要?] + 保留窗口内 tail 原文（逐字不变） + 当前回合新增`；被移出的 `archived_prefix` → 归档行。`compress_tool_message_contents`（[transforms.rs](file:///home/gzz/crabmate/server/src/cm_agent/message_pipeline/transforms.rs#L103-L122)）与 reasoning 剥离**仅视图层、不回写 canonical**。
3. **净化链**复用（§3.7.1），**保留** `CRABMATE_CONTEXT_SUMMARY_NAME` 与首轮画像；
4. canonical 至多保留最近 1 条 artifact 旁注，旧旁注以 `reason = 'artifact'` 移入归档（§3.9）。

**A3 — 原子落盘**：把 [`save_if_revision`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L233-L291) 由单条 UPDATE 扩展为「canonical UPDATE + 归档 INSERT + revision+1 **同事务**」；`run_agent_turn` 的 `StructuralCommit` 随 messages 一并透传到 [`post_turn_web_prepare_and_save`](file:///home/gzz/crabmate/server/src/chat_job_queue/stream_finish.rs#L34-L76) 落库；内存后端（[app_state.rs](file:///home/gzz/crabmate/server/src/web/app_state.rs#L478) `ConversationBacking::Memory`）给出等价处理并在文档标注差异。

**A4 — `list_all_messages_json` 收口**（与 A2 同 PR，堵不可逆清理窗口）：[`list_all_messages_json`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L332-L341) 追加读归档表（解压并入），使 [`referenced_upload_filenames`](file:///home/gzz/crabmate/server/src/web/app_state.rs#L531-L556) 不误删被归档消息引用的 `/uploads/` 文件（§9.2 item6）。

**A5 — derive 退化 + 回归**：`derive` 语义变为「读已基线化的 canonical」，不再额外应用 artifact；回归见 §5.3.5。

#### 5.3.3 Phase B 工程拆解（已实施）

1. 抽出**只读模型请求视图构建**入口（dry-run 压缩，不落盘；**默认不触发 LLM 摘要**，§3.10）；
2. `builtin_btw` 改走该入口；
3. 回归（§9.2 item7）：btw 长会话 `before_tokens` 有界，且**不写** canonical / 归档。

#### 5.3.4 Phase C 工程拆解（C1 / C2 / C3 / C4 全部已实施）

**C1 — 分支锚点改造（P0，最易踩坑，已实施）**：落点 [`truncate_before_user_ordinal_if_revision`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L435-L466) + [`branch_truncation_cut_index`](file:///home/gzz/crabmate/server/src/web/app_state.rs#L455-L466) 及分支入口。改为按**会话级 `user_turn_ordinal`（含归档、跨归档稳定）**定位（§3.11），不再用 canonical 内下标；分支点落回归档前缀时先 **re-expand**（`load_archive_messages` 按 `seq` 重放回 canonical）再截断，并**同事务清除该会话归档**；分支点**早于保留窗口** → 返回**显式错误**（不静默截错）。

**C2 — 导出携带归档（已实施）**：`GET /conversation/messages` 新增可选查询参数 `include_archive`（boolean，默认 `false`）与可选响应字段 `archive`（`skip_serializing_if = "Vec::is_empty"`，默认不返回以保持体积与旧客户端兼容）。落点：契约 [`ConversationMessagesQuery`](file:///home/gzz/crabmate/server/src/cm_api_contract/chat.rs) / [`ConversationMessagesResponseBody`](file:///home/gzz/crabmate/server/src/cm_api_contract/chat.rs)；读归档 [`load_conversation_archive_messages`](file:///home/gzz/crabmate/server/src/web/app_state.rs)（仅 SQLite 有归档，内存后端返回空，失败容错返回空）；handler [`conversation_messages_handler`](file:///home/gzz/crabmate/server/src/web/chat_handlers/chat/mod.rs)；OpenAPI 参数 [`openapi_paths.rs`](file:///home/gzz/crabmate/server/src/web/openapi/openapi_paths.rs) 与快照 `docs/openapi.json` 同步。`archive` 与 `messages` 同为 `WebClientSnapshotMessage` 形状（复用泛型响应体 `<M>`）。回归：契约解析、app_state 内存 / SQLite / 不存在会话、OpenAPI schema 与空值省略。

**C3 — 配置与文档（已实施）**：新增并同步 `docs/配置说明.md` / `docs/en/CONFIGURATION.md` / `config/default_config.toml` / `CHANGELOG.md`：`context_archive_max_rows_per_conversation`（默认 256，`0` = 不限）、`context_archive_codec_min_bytes`（默认 4096）、`context_btw_summary_enabled`（默认 `false`，旁路摘要开关，见 §3.10）；同步 `cm_config/{builder,env_overrides,validate,hot_reload}` 与 `finalize_*`。`context_baseline_*` **已确认不再需要（废弃）**（见 §5.3.6）。C3 本身无新增导出字段；导出字段变更由 C2 引入并已同步 OpenAPI。

**C4 — `prune` 覆盖归档（已实施）**：[`prune`](file:///home/gzz/crabmate/server/src/conversation_store.rs#L300-L324) 删除会话 / 超限时同事务删除归档行。

#### 5.3.5 验证矩阵（映射 §9.2）

| 验收项 | 阶段 | 关键断言 |
|---|---|---|
| item1 / item2 跨回合收敛 + canonical 封顶 | A | 3+ 回合后 `before_tokens` 不回升；≥50 回合 `len` 稳定 |
| item3 artifact 至多 1 条 | A | 旧旁注可在归档按 `reason='artifact'` 查得 |
| item4 原文可回放（含 gzip） | A | 解压后逐字一致 |
| item5 分支截断 + re-expand | C | 超窗分支返回显式错误 |
| item6 上传引用 | A | 归档消息引用的 `/uploads/` 不被清 |
| item7 旁路 | B | btw 不写 canonical / 归档 |
| item8 净化链 | A | 无注入残留、保留摘要与首轮画像 |
| item9 旧 Client 兼容 | A / C | 软字段形状不变 |
| item10 结构性提交 | A | 保留窗口原文逐字比对未变 |

验证命令：`cargo test --lib context_` / `--lib conversation_store`；提交前 `cargo clippy --all-targets --all-features -- -D warnings` + `cargo fmt --all` + `pre-commit run --all-files`。

#### 5.3.6 关键风险与待确认

- **R-A2（最大）**：现有压缩函数只返回计数、丢失被移出原文，需把「前缀捕获」贯穿 `prepare_messages_for_model` 全链路——Phase A 的主要工作量；
- **R-A3**：`run_agent_turn` 无 DB 句柄，归档行须经 `stream_finish` 保存路径透传；需定夺「扩展 `save_if_revision`」还是「新增 `save_with_archive_if_revision`」；
- **R-内存后端**：`ConversationBacking::Memory` 是否也持久化归档（建议：仅内存维护、重启失效，并在文档标注差异）；
- **已收口（2026-10-16）**：btw 摘要开关定为 `context_btw_summary_enabled`（默认 `false`，`[agent]` 扁平键，见 §3.10）；`context_baseline_*` **确认不再需要（废弃）**——基线摘要复用既有 `context_summary_*` 配置，不引入独立基线键。归档 `range_*` 已明确**仅作审计快照**、不用于回放定位（§3.8）。

---

## 6. Consequences & Risks

### 6.1 Positive

- 工具调用多不再等同于上下文消耗高；
- UI 能区分工具输出压缩与历史丢弃；
- 裁剪触发可由 Token 数解释和复现；
- 工具调用消息结构在裁剪后保持合法；
- 完整历史可审计、导出与重新生成模型视图；
- 后续可比较不同模型、tokenizer 与压缩策略的效果；
- （跨回合）压缩成果跨回合稳定保持，水位不再“越压越大”。

### 6.2 Negative

- 最终请求 Token 估算会增加 CPU 成本；
- tools JSON 与附件计量需要供应商适配；
- 完整历史与模型视图分离会增加数据结构和测试复杂度；
- LLM 摘要具有额外费用、延迟与事实遗漏风险；
- 供应商 usage 通常在请求完成后才可得，只能用于校准下一次调用；
- Server 与 Client 需跨仓同步契约、文案和测试。

### 6.3 Operational Impact

- Phase 1 会提高默认历史保留量，可能增加请求体积、延迟与费用；
- 上线后需观察输入 Token、首字节延迟、上下文超限错误与摘要次数；
- 应保留配置回退到旧 `max_message_history` 的能力；
- 未完成 Phase 2 前，不应把 Phase 1 宣称为“精确 Token 裁剪”。

### 6.4 Risks（跨回合方案）

| 风险 | 说明 | 缓解 |
|---|---|---|
| 归档写放大 | 大文件反复读入后被归档，磁盘占用上升 | `messages_json` 在字节数 ≥ `context_archive_codec_min_bytes` 时 `codec = gzip`（复用 `flate2`，见 §3.8）；归档保留上限 `context_archive_max_rows_per_conversation` + TTL 与 `prune` 联动回收 |
| 就地改写绕过归档（P1） | 工具正文压缩 / reasoning 剥离是**视图层就地 mutate**，不在任何“被移出前缀”区间内，若误回写 canonical 会破坏「归档 ∪ canonical 原文 = 完整历史」不变式 | reconcile **只提交结构差异**，视图层内容改写不回写 canonical（见 §3.7）；补测“压缩后回放仍得逐字原文” |
| 分支点早于归档保留窗口（P1） | `prune` / 回收已删早期归档行后，落到该区间的分支点**无法 re-expand**，截断无从定位 | 分支接口对超窗分支点返回**显式错误**（不静默截错，见 §3.8 / §3.11）；提高保留上限或对关键会话延长 TTL |
| 归档早于扫描收口（P0） | Phase A 写归档而扫描仍只读 canonical → `/uploads/` 文件被判孤儿误删 | `list_all_messages_json` 必须与归档写入**同 PR（Phase A）**；见 §4.2 / §9.2 验收项 6 |
| 回归面变宽 | canonical 由“只增”变“可变”，触及截断 / 导出 / 扫描 | Phase A 只改**提交、派生与 `list_all_messages_json` 收口**；Phase C 再动截断 / 导出 / `prune`；每步补回归 |
| 净化链遗漏 | reconcile 把临时注入固化进 `canonical` | §3.7.1 显式复用落盘剥离链；§9.2 验收项 8 |
| 摘要丢失关键事实 | 基线摘要成为 canonical 后原文不再直接可见 | 归档可回放；摘要质量由既有 `context_summary_covers_anchors` 类验收夹具守护 |
| 与既有原则的沟通成本 | §3.4 曾被引用为“不可删历史” | §1.5 / §3.4 显式修订该条措辞 + CHANGELOG 说明 |
| 半提交 | canonical / 归档 / artifact 旁注 分多次写导致不一致 | 强制同 revision 同事务（§4.2） |

---

## 7. Alternatives Considered

### 7.1 仅把 `max_message_history` 从 24 调大

不采纳为最终方案。

优点是改动小；缺点是短消息可能长期不裁，单条大工具输出仍可能溢出，而且不同任务的消息粒度差异很大。仅作为 Phase 1 缓解措施。

### 7.2 仅使用近似字符预算

不采纳为主策略。

字符估算实现简单，但无法准确反映模型 tokenizer、工具 schema、图片和供应商封装开销。保留为未知模型的降级路径。

### 7.3 每条消息独立按 Token 删除

不采纳。

它容易破坏 `assistant.tool_calls` / `tool` 配对，并丢失完整 ReAct 行为语义。

### 7.4 每次接近上限都调用 LLM 摘要

不采纳为唯一策略。

摘要会增加费用、延迟和幻觉风险。应先执行确定性的工具输出压缩，再按需摘要历史中段。

### 7.5 Client 自行计算和决定裁剪

不采纳。

Client 缺少最终 tools、供应商消息变换和 Server 动态注入的完整信息。Server 必须是预算与压缩决策真源。

### 7.6 止血版：`derive` 应用 artifact，但不改 canonical（不采纳为最终方案）

让 `derive` 读最后一条 artifact，按 `summarized_canonical_range` / `removed_turn_group_ranges` 重建视图。

- **优点**：改动最小（单函数），不动持久化层，风险低；
- **缺点**：**只解决 R2**。`canonical` 仍单调增长（R1 未消），`to_vec()` 仍全量克隆，artifact 旁注继续累积，旁路仍不压（R3 未消）。属于“对症止血”，`before_tokens` 的持续抬升不会消失。
- **定位**：作为 Phase A 落地前的**热修**保留（若线上水位抖动严重）；本 ADR 的正式方案仍走 §3.7–§3.10。

### 7.7 每次调用前重建整表（不采纳）

每回合把 canonical 重写为“压缩后形态 + 归档”。语义等价于 §3.7，但把归档做成了“当前 canonical 的副本”，无法区分“历史原文”与“当前基线”，审计性更差。§3.7 的“区间归档 + 会话级 `user_turn_ordinal` 锚点（§3.11）”更精确。

### 7.8 只用内存压缩、不落盘（不采纳）

压缩成果不跨进程/跨回合保留，重启即失效，且与“可审计”目标背离。

---

## 8. Resolved Questions

1. 安全余量采用可配置固定 Token，并同时扣除输出预留；
2. 图片先使用保守固定预算，供应商返回 usage 时在最终快照标记校准来源；
3. tool schema 按每次真实下发工具集计数；
4. reasoning 按现有供应商 round-trip 规则进入消息估算；
5. `ConversationTurnGroup` 以真实 user 为边界，并行 tool calls 保持同组；取消回合不提交派生视图；
6. Token 统计以兼容方式扩展现有 `tiktoken_prompt_tokens`，不提升 SSE 版本；
7. 规范历史沿用会话 revision；每次模型调用的 `ModelContextArtifact.schema_version = 1` 作为隐藏旁注随同一 revision 原子保存，`model_call_sequence` 区分回合内调用。

---

## 9. Verification

### 9.1 Token 主导压缩的回归场景（Phase 1–3）

1. 64 条边界前后行为；
2. 仅工具输出压缩时不显示“已裁剪历史”；
3. 条数或字符裁剪时仍显示“已裁剪历史”；
4. 多次 tool call 的完整组不被拆断；
5. tools JSON 增大时 Token 预算相应增长；
6. 未知模型走保守估算；
7. Client 用量分母与 Server `max_input_tokens` 一致；
8. 旧 Client 忽略新增软字段且仍可显示 timeline。

### 9.2 跨回合压缩的回归场景

1. **跨回合收敛**：连续 3+ 回合触发压缩后，第 4 回合的 `before_tokens` **不得**回升到首压之前的量级；
2. **canonical 封顶**：长会话（≥ 50 回合）后 `canonical.len()` 稳定在阈值量级，不再单调增长；
3. **artifact 旁注不累积**：`canonical` 中 `crabmate_model_context_artifact` **至多 1 条**；被替换的旧旁注可在归档中按 `reason = 'artifact'` 查到；
4. **原文可回放**：对被归档区间调回放，内容与归档前逐字一致（含 `codec = gzip` 时解压后逐字一致）；
5. **分支截断 + re-expand**：`truncate_before_user_ordinal_*` 在基线化会话上按**会话级 `user_turn_ordinal`（含归档）**定位正确；分支点落回归档前缀时先 **re-expand** 还原完整原文再截断，且**同事务清除该会话归档**；回滚后再次前进不产生“已删归档被引用”的空洞；分支点早于保留窗口时返回**显式错误**（见 §3.11）；
6. **上传引用**：被归档消息引用的 `/uploads/` 文件**不被**判为孤儿（Phase A 起即须通过）；
7. **旁路**：`builtin_btw` 长会话走同步压缩（**默认无 LLM 摘要调用**），且**不写** canonical / 归档；
8. **净化链**：reconcile 提交后，`canonical` 中不含长期记忆 / 变更集 / 编排注入 user / 执行约束 system；但**保留** `CRABMATE_CONTEXT_SUMMARY_NAME` 基线摘要与首轮工作区画像；
9. **旧 Client**：`GET /conversation/messages` 与 `context_artifacts` 软字段形状不变；
10. **结构性提交**：reconcile 后对保留窗口内的原文逐字比对 canonical，确认未被就地改写（工具正文压缩 / reasoning 剥离只作用于视图）；「归档 ∪ canonical 原文 = 完整原始历史」不变式成立（见 §3.7 / §3.8）。

### 9.3 验证命令

```bash
# Server
cargo test -p crabmate --lib context_
cargo test -p crabmate --lib conversation_store
bash scripts/check-sse-protocol.sh

# Client（无协议变更，仅回归）
cd ../client
make frontend-check
```

---

## 10. References

- [OpenHands Condenser](https://docs.openhands.dev/sdk/arch/condenser)：历史与模型视图、滚动压缩、Token-aware 演进
- [Aider `ChatSummary`](https://github.com/Aider-AI/aider/blob/main/aider/history.py)：Token 触发、保留尾部、递归摘要
- [Cline Context Management](https://deepwiki.com/cline/cline/3.5-context-management)：安全输入预算、压缩触发与目标比例
- [LangChain `trim_messages`](https://reference.langchain.com/python/langchain-core/messages/utils/trim_messages)：Token 计数与消息起止结构约束

---

## 11. 修订记录

| 日期 | 变更 |
|---|---|
| 2026-08-24 | `context_window_token_aware_compaction.md` 初稿（Phase 1–3 已实施） |
| 2026-10-15 | `context_window_cross_turn_compaction.md` 初稿（Proposed）：canonical 基线化 + 原文归档 + 旁路收口；修订 Token 主导压缩 ADR §3.4 边界 |
| 2026-10-15 | 跨回合 ADR 第二稿（评审修订）：把 `list_all_messages_json` 收口提到 Phase A（堵 P0 孤儿文件窗口）；新增提交前净化链；明确 artifact 旁注回收（canonical 至多 1 条）；定义 `archived_through_seq` 为可安全裁撤上界；归档增加 `codec` 压缩列与保留上限；btw 默认不触发 LLM 摘要；增补净化链与分支回滚验收 |
| 2026-10-15 | 合并：将上述两篇 ADR 合并为本文（`context_window_compaction.md`），统一编号、去重原则表述，并删除原两文件 |
| 2026-10-15 | 合并后复核修订（P0/P1/P2）：① 分支锚点由 canonical 内序号改为**会话级 `user_turn_ordinal`（含归档）**并新增 §3.11 re-expand，取代 `archived_through_seq` 水位（P0-1/P0-2）；② §3.7 明确 reconcile **只提交结构差异**，工具正文压缩 / reasoning 剥离仅视图层（P1-3）；③ 归档 `codec` 由 `zstd` 改为 `gzip`（复用 `flate2`）并定义 `context_archive_codec_min_bytes` 选择阈值，`range_*` 限定为审计快照（P1-4/P2-5/P2-7）；④ 「只追加 / 永久可查」改为「保留窗口内可回放」+ 超窗显式错误（P2-6）；⑤ 同步 §4.1、§4.2、§5.2、§6.4、§9.2 |
| 2026-10-15 | 新增 §5.3 实施拆解（§5.2 工程附录）：按 Phase A–C 细化文件级落点、顺序与依赖（A0/A1 先于 A2）、验证矩阵（映射 §9.2）与关键风险；仅细化实施，不改变 §3–§4 契约 |
| 2026-10-16 | Phase A 已实施并合入（归档表 + 读写 API、结构性提交 reconcile、同事务原子落盘、`list_all_messages_json` 合并归档、`derive` 直读 canonical）；状态头、§4.2 阶段列与 §5.2/§5.3.2 标注「已实施」，并记录两处临时口径（归档前缀分支返回 `Conflict`、`codec` 阈值暂为常量）以待 Phase B/C 收口 |
| 2026-10-16 | Phase B（旁路收口）已实施：新增只读入口 `prepare_readonly_model_messages`（dry-run 压缩、默认不触发 LLM 摘要、无 store 参数）并抽出共用助手 `sync_trim_and_estimate_before` / `finalize_token_budget_compaction`；`builtin_btw` 改走该入口；补回归 `readonly_entry_bounds_tokens_without_summary_or_persistence`；状态头、§5.2 与 §5.3.3 标注「已实施」。Phase C 待实施 |
| 2026-10-16 | Phase C 部分实施：**C1**（分支锚点改用会话级 `user_turn_ordinal` + re-expand + 超窗显式错误 + 同事务清归档）、**C4**（`prune` 同事务级联删除 / 回收归档行）、**C3**（新增两键 `context_archive_max_rows_per_conversation` 默认 256 / `context_archive_codec_min_bytes` 默认 4096，同步 `cm_config` 管道 + `config/default_config.toml` + `配置说明.md` + `CONFIGURATION.md` + `CHANGELOG.md` 并补回归）。`context_baseline_*` 与 btw 摘要开关因键名 / 默认值待定**本轮未落**（§5.2 / §5.3.4 / §5.3.6 已标注）；状态头改为「实施中：C1/C3/C4 已实施，C2 待实施」。**C2（导出携带归档）** 与状态改 Accepted 待后续 |
| 2026-10-16 | Phase C 收尾：**C2（导出携带归档）** 已实施——`GET /conversation/messages` 新增可选查询参数 `include_archive`（默认 `false`）与可选响应字段 `archive`（`skip_serializing_if = "Vec::is_empty"`，默认不返回以保持体积与旧客户端兼容）；读归档走 `AppStateConversationRuntime::load_conversation_archive_messages`（仅 SQLite 有归档，内存后端返回空，失败容错返回空）并经 `WebChatAppFacet` 委派；`archive` 与 `messages` 同为 `WebClientSnapshotMessage` 形状复用泛型响应体 `<M>`；同步 `docs/openapi.json`（`include_archive` 参数 + `archive` schema）与 `docs/命令行与路由.md`；补契约 / app_state / OpenAPI 回归。Phase C **C1–C4 全部落地**，本文状态改为 **Accepted** |
| 2026-10-16 | 收口 §5.3.6 两项待定：① 旁路摘要开关定为 `context_btw_summary_enabled`（默认 `false`，`[agent]` 扁平键）——只读入口 `prepare_readonly_model_messages` 改为 **async** 并接受可选 `ReadonlySummaryLlm`，`answer_btw` 按该键门控；② `context_baseline_*` **确认不再需要（废弃）**——基线摘要复用既有 `context_summary_*` 配置。补齐 §9.2 验收测试：item1（跨回合收敛）、item2（canonical 封顶，60 回合）、item6（归档消息引用的 `/uploads/` 不判孤儿）、item7（`context_btw_summary_enabled` 默认关闭）、item8（抽取纯函数 `strip_persist_injections`，补 `is_execution_constraint_ephemeral_system` 剔除，断言保留摘要与首轮画像）、item10（归档 ∪ canonical 原文 = 完整原始历史）；同步 §3.10 / §5.3.3 / §5.3.4 / §5.3.6 |
