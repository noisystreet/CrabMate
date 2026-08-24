# ADR：上下文窗口采用 Token 主导、交互组完整的压缩策略

**状态**：Accepted（Phase 1 已实施；Phase 2–3 待实施）  
**日期**：2026-08-24  
**决策范围**：Server 上下文计量、模型请求视图、裁剪/压缩事件；Client 用量与时间线展示  
**关联文档**：

- [`context_trimming_scheme.md`](./context_trimming_scheme.md)：当前会话同步管道事实说明
- [`context_window_management_react_pruning.md`](./context_window_management_react_pruning.md)：ReAct 长循环的早期设计稿
- [`../SSE协议.md`](../SSE协议.md)：`timeline_log` / `context_trim` 协议
- [`../配置说明.md`](../配置说明.md)：上下文配置真源

---

## 1. Context

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

---

## 2. Decision Drivers

按优先级排序：

1. 模型请求必须稳定落在真实输入窗口内；
2. 不得破坏 `assistant.tool_calls` 与 `tool` 的结构关系；
3. 少量工具型回合不应仅因消息条数较多而提前删除历史；
4. 持久化会话应可审计，不应等同于压缩后的模型视图；
5. Server 与 Client 应使用可解释、尽量一致的预算口径；
6. 改造须分阶段兼容现有 SSE、SQLite 会话与旧 Client；
7. 无供应商精确计数能力时仍须有保守、确定性的降级路径。

---

## 3. Decision

### 3.1 Token 预算成为主要触发条件

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

### 3.2 按完整交互组裁剪

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

### 3.3 分级压缩

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

### 3.4 持久化历史与模型视图分离

长期目标是：

- `ConversationHistory`：SQLite 中的完整、可审计历史；
- `ModelContextView`：每次模型调用前派生的压缩视图；
- `ContextCompactionReport`：描述本次视图如何生成。

裁剪不得永久删除规范会话历史。摘要可作为派生工件持久化，但不能替代原始消息。

在完成分离前，现有就地 `Vec<Message>` 管道继续作为兼容实现；新增逻辑不得进一步扩大其职责。

### 3.5 计量与展示口径

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

### 3.6 事件语义拆分

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

---

## 4. Delivery Plan

### Phase 1：缓解提前裁剪并修正文案（已实施）

范围：

1. 默认 `max_message_history` 从 24 调整为 64；
2. 保留环境变量与 TOML 覆盖能力；
3. 仅 `compress_hits > 0` 时显示“已压缩工具输出”；
4. 历史确实被删除或摘要时才显示“已裁剪历史”；
5. Client 设置文案由“注入与窗口裁剪”调整为“上下文注入与压缩”；
6. 增加回归测试，覆盖工具压缩与历史裁剪标题分流；
7. 同步配置文档、SSE 文档和 CHANGELOG。

本阶段不改变裁剪算法与 JSON 形状，可独立回滚。

### Phase 2：Token 主导与交互组完整性

范围：

1. 建立最终请求 Token 预算器；
2. 计入 tools JSON、system、消息和附件预算；
3. 实现 `ConversationTurnGroup`；
4. 以 Token 阈值触发压缩；
5. 将 `max_message_history` 变为高位安全兜底；
6. 输出 `ContextCompactionReport`；
7. 用工具密集型 fixture 验证 2～4 个用户回合不会无故裁剪。

### Phase 3：历史与模型视图分离

范围：

1. SQLite 保持完整规范历史；
2. 每次调用派生 `ModelContextView`；
3. 摘要与被移出片段可回放；
4. Server/Client 统一显示可用输入预算；
5. 支持供应商 usage 校准与估算来源标记。

---

## 5. Consequences

### Positive

- 工具调用多不再等同于上下文消耗高；
- UI 能区分工具输出压缩与历史丢弃；
- 裁剪触发可由 Token 数解释和复现；
- 工具调用消息结构在裁剪后保持合法；
- 完整历史可审计、导出与重新生成模型视图；
- 后续可比较不同模型、tokenizer 与压缩策略的效果。

### Negative

- 最终请求 Token 估算会增加 CPU 成本；
- tools JSON 与附件计量需要供应商适配；
- 完整历史与模型视图分离会增加数据结构和测试复杂度；
- LLM 摘要具有额外费用、延迟与事实遗漏风险；
- 供应商 usage 通常在请求完成后才可得，只能用于校准下一次调用；
- Server 与 Client 需跨仓同步契约、文案和测试。

### Operational Impact

- Phase 1 会提高默认历史保留量，可能增加请求体积、延迟与费用；
- 上线后需观察输入 Token、首字节延迟、上下文超限错误与摘要次数；
- 应保留配置回退到旧 `max_message_history` 的能力；
- 未完成 Phase 2 前，不应把 Phase 1 宣称为“精确 Token 裁剪”。

---

## 6. Alternatives Considered

### 6.1 仅把 `max_message_history` 从 24 调大

不采纳为最终方案。

优点是改动小；缺点是短消息可能长期不裁，单条大工具输出仍可能溢出，而且不同任务的消息粒度差异很大。仅作为 Phase 1 缓解措施。

### 6.2 仅使用近似字符预算

不采纳为主策略。

字符估算实现简单，但无法准确反映模型 tokenizer、工具 schema、图片和供应商封装开销。保留为未知模型的降级路径。

### 6.3 每条消息独立按 Token 删除

不采纳。

它容易破坏 `assistant.tool_calls` / `tool` 配对，并丢失完整 ReAct 行为语义。

### 6.4 每次接近上限都调用 LLM 摘要

不采纳为唯一策略。

摘要会增加费用、延迟和幻觉风险。应先执行确定性的工具输出压缩，再按需摘要历史中段。

### 6.5 Client 自行计算和决定裁剪

不采纳。

Client 缺少最终 tools、供应商消息变换和 Server 动态注入的完整信息。Server 必须是预算与压缩决策真源。

---

## 7. Open Questions

在 Phase 2 实现前需确定：

1. 安全余量采用固定 Token、比例，还是取两者较大值；
2. 图片与多模态输入的供应商预算接口；
3. tool schema Token 是否按每次真实下发工具集缓存；
4. reasoning 正文在不同供应商下的保留与计量策略；
5. `ConversationTurnGroup` 对并行 tool calls 与中途取消的边界；
6. Token 统计字段落在现有 `tiktoken_prompt_tokens` 扩展还是新版本快照；
7. Phase 3 的完整历史、摘要工件与会话 revision 如何共同版本化。

---

## 8. Verification

至少建立以下回归场景：

1. 64 条边界前后行为；
2. 仅工具输出压缩时不显示“已裁剪历史”；
3. 条数或字符裁剪时仍显示“已裁剪历史”；
4. 多次 tool call 的完整组不被拆断；
5. tools JSON 增大时 Token 预算相应增长；
6. 未知模型走保守估算；
7. Client 用量分母与 Server `max_input_tokens` 一致；
8. 旧 Client 忽略新增软字段且仍可显示 timeline。

跨仓验证：

```bash
# Server
cargo test -p crabmate --lib context_
bash scripts/check-sse-protocol.sh

# Client
cd ../client
make frontend-check
```

---

## 9. References

- [OpenHands Condenser](https://docs.openhands.dev/sdk/arch/condenser)：历史与模型视图、滚动压缩、Token-aware 演进
- [Aider `ChatSummary`](https://github.com/Aider-AI/aider/blob/main/aider/history.py)：Token 触发、保留尾部、递归摘要
- [Cline Context Management](https://deepwiki.com/cline/cline/3.5-context-management)：安全输入预算、压缩触发与目标比例
- [LangChain `trim_messages`](https://reference.langchain.com/python/langchain-core/messages/utils/trim_messages)：Token 计数与消息起止结构约束
