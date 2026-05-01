# 上下文裁剪方案设计

**状态**：与当前实现同步的设计说明（会话同步管道以源码为准；分层 ReAct 专用裁剪另见下文关联文档）。  
**受众**：维护 `agent::message_pipeline`、`agent::context_window` 与 Agent 主循环的开发者。  
**权威实现**：`src/agent/message_pipeline.rs`（**`apply_session_sync_pipeline`** 顺序契约）、`src/agent/context_window.rs`（**`prepare_messages_for_model`**、**`maybe_summarize_with_llm`**）。  
**配置与环境变量**：键名与默认值以 **`docs/配置说明.md`**、`config/default_config.toml` 为准。

---

## 1. 目的与范围

### 1.1 文档目的

归纳 CrabMate **发往模型前**对会话 **`messages`** 的裁剪与压缩策略的设计要点：动机、分层、固定顺序、观测方式及演进边界，便于评审配置变更与新增管道步骤。

### 1.2 范围

| 包含 | 不包含（另见） |
|------|----------------|
| 进程内 `Vec<Message>` 同步变换（tool 压缩、条数/字符裁剪、孤立 tool、合并 assistant） | HTTP 层 **`stream_chat`** 内与其它绕过管道的拼装（应避免新增此类路径） |
| 可选「中间段 LLM 摘要」（**`maybe_summarize_with_llm`**） | 供应商网关适配（**`conversation_messages_to_vendor_body`**、vendor adapter）的完整枚举——仅描述其与裁剪的衔接关系 |
| **`GET /status`** 计数器与日志字段 | 前端展示策略 |

### 1.3 非目标

本文件**不**替代 `message_pipeline.rs` 模块内注释；步骤顺序以源码 **`MessagePipelineStage`** 与 **`apply_session_sync_pipeline`** 为准。

---

## 2. 为何需要裁剪

1. **上下文上限**：消息总长超过模型窗口会导致报错、截断或隐性丢失末尾内容。  
2. **成本与延迟**：输入 token 与费用、首字延迟正相关。  
3. **工具输出体积**：`role: tool` 正文（命令输出、大 JSON）往往远大于自然语言轮次，若不先压缩再删条，易出现「条数很少但仍撑爆窗口」。  
4. **信噪比**：远期轮次对当前决策贡献通常低于近期轮次；在保留尾部的前提下压缩中部，是常见权衡。

---

## 3. 两阶段架构（必须区分）

设计上分为两步，**职责不同，勿混为一谈**：

### 3.1 会话同步（Session sync）

- **入口**：**`message_pipeline::apply_session_sync_pipeline`**（由 **`context_window::prepare_messages_before_model_call_sync`** / **`prepare_messages_for_model`** 调用）。  
- **作用对象**：进程内会话 **`Vec<Message>`**（就地修改）。  
- **目标**：控制长度、压缩工具噪声、维护对话结构合法性（孤立 tool、相邻 assistant 等）。

### 3.2 供应商出站（Vendor body）

- **入口**：**`conversation_messages_to_vendor_body`** / **`normalize_stripped_messages_for_vendor_body`** 等（见 `message_pipeline.rs`）。  
- **作用对象**：从会话切片构造 **`ChatRequest.messages`**，**不写回**会话 `Vec`。  
- **典型处理**：跳过 UI 分隔线 / 长期记忆注入展示条、按网关处理 **`reasoning_content`**、OpenAI 兼容 **normalize**、可选 **system→user** 折叠。

**原则**：会话侧裁剪解决「体积与结构」；出站路径解决「供应商兼容」。新增裁剪逻辑时默认落在 **会话同步**，除非明确只做 HTTP 层语义且不应改写会话 truth。

---

## 4. 会话同步管道顺序（契约）

以下顺序**固定**，实现见 **`apply_session_sync_pipeline`**；新增步骤须同步更新：

1. **`MessagePipelineStage`** 枚举  
2. **`message_pipeline.rs` 模块文档**编号列表  
3. **`docs/开发文档.md`**「上下文窗口策略」  

当前顺序（与源码一致）：

1. 记录起点快照（观测）  
2. **`compress_tool_message_contents`**（**`tool_message_max_chars`**）  
3. **`trim_messages_by_count`**（**`max_message_history`**）  
4. 若 **`context_char_budget > 0`**：**`trim_messages_by_char_budget`** → 再次 **`compress_tool_message_contents`**  
5. **`drop_orphan_tool_messages`**  
6. **`merge_consecutive_assistants_in_place`**

**设计意图简述**：

- **先压 tool 再按条数删**：避免「轮次少了但单条 tool 仍极大」。  
- **字符预算后再压一遍 tool**：删旧消息后剩余 tool 仍可能触线。  
- **最后处理 orphan 与 merge**：在删条之后统一修正结构。

---

## 5. 可选 LLM 摘要（中间段压缩）

当 **`context_summary_trigger_chars > 0`** 且非 system 文本总字符超过阈值时，**`maybe_summarize_with_llm`** 可发起**无 tools** 的 `chat/completions`，将「中间段」折叠为一条 **user** 摘要，尾部保留 **`context_summary_tail_messages`** 条（细节见 **`context_window.rs`**）。

**设计要点**：

- **额外成本与失败语义**：摘要是一次独立 LLM 调用，失败策略需在实现中保持可预期（勿静默丢历史）。  
- **幻觉风险**：摘要可能遗漏或歪曲约束；通过尾部保留条数、触发阈值与提示词约束缓解。  
- **与同步管道的关系**：摘要通常在同步裁剪**之后**执行（见 **`prepare_messages_for_model`** 调用链），具体顺序以源码为准。

---

## 6. 设计要点清单（评审用）

实现或改配置时建议逐项核对：

### 6.1 对话合法性

- **`tool_calls` / `role: tool`** 配对完整；删中间消息后不残留悬空 tool。  
- 裁剪后仍满足 **`normalize_messages_for_openai_compatible_request`** 一类契约。

### 6.2 预算口径

- **条数**（**`max_message_history`**）与 **近似字符**（**`context_char_budget`**）互补：条数控制轮次规模，字符抑制超长单条；二者与真实 token 仅为近似关系。  
- 若未来引入 **token 级预算**，应与现有顺序分层定义，避免多重裁剪冲突。

### 6.3 保留优先级（策略层）

- **System / 用户目标 / 近期轮次**通常优先于远古轮次。  
- **失败证据、验收结果、审批结论**是否必须留在窗口——业务上若关键，应在规则或摘要提示词中显式强调。

### 6.4 工具正文 vs 自然语言

- 优先压缩 **`crabmate_tool`** 信封内 **`output`**（首尾采样 + **`output_truncated`** 等元数据），非信封路径按前缀截断；便于模型知晓「保留比例」而非误以为全文可用。

### 6.5 多入口一致性

- Web / CLI / 分阶段 / 分层等路径应统一经 **`prepare_messages_for_model`**（或等价入口），避免同一会话在不同入口「模型所见不同」。

### 6.6 可观测性

- **`MESSAGE_PIPELINE_COUNTERS`**：**`trim_count_hits`**、**`trim_char_budget_hits`**、**`tool_compress_hits`**、**`orphan_tool_drops`**；**`GET /status`** 暴露累计值（进程级，非单会话）。  
- **日志**：**`crabmate::message_pipeline=trace`** 输出逐步 **`session_sync_step`**。

### 6.7 安全与脱敏

- 裁剪与日志不得输出完整密钥或整条 Authorization；工具正文日志遵循 **`secrets-and-logging`** 仓库规则。

### 6.8 厂商与出站衔接

- **`reasoning_content`**、**`fold_system`** 等在出站阶段处理；会话侧勿过早丢弃下游仍需要的字段，除非已与 **`LlmVendorAdapter`** 策略对齐。

---

## 7. 配置索引（查阅用）

具体默认值、环境变量前缀与 clamp 范围以 **`docs/配置说明.md`** 为准，常用键包括：

- **`tool_message_max_chars`** / **`CM_TOOL_MESSAGE_MAX_CHARS`**  
- **`max_message_history`** / **`CM_MAX_MESSAGE_HISTORY`**  
- **`context_char_budget`** / **`CM_CONTEXT_CHAR_BUDGET`**  
- **`context_summary_trigger_chars`**、**`context_summary_tail_messages`** 等 **`CM_CONTEXT_*`**

---

## 8. 与其它设计文档的关系

| 文档 | 关系 |
|------|------|
| **`docs/design/context_window_management_react_pruning.md`** | 面向 **`hierarchy`** Operator **ReAct** 的长循环裁剪（**状态：待实现**）；与本文全局 **`message_pipeline`** 互补，落地时应复用 **`context_window`** / **`message_pipeline`** 能力，避免第二套条数字符逻辑。 |
| **`docs/design/agent_state_management.md`** | 更广义的会话/产物状态；与裁剪正交。 |
| **`docs/开发文档.md`** | 维护者索引与「上下文窗口策略」段落；本文展开设计要点，细节仍以源码为准。 |

---

## 9. 演进建议（非承诺）

1. **分层 ReAct**：实现 **`context_window_management_react_pruning.md`** 时优先 token 估算与 **`prepare_messages_for_model`** 挂钩。  
2. **契约测试**：固定 fixture 覆盖「条数边界、超长 tool、孤儿 tool、摘要触发」等回归场景。  
3. **长期记忆 / 向量检索**：与裁剪顺序协调，避免重复注入或互相覆盖（参见 **`docs/待办清单.md`** 记忆相关条目）。

---

## 10. 修订记录

| 日期 | 摘要 |
|------|------|
| 2026-05-01 | 初版：会话同步契约、两阶段架构、设计要点与关联文档。 |
