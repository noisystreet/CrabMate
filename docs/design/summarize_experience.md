# 设计文档：经验总结工具 `summarize_experience`

## 1. 背景与目标

### 1.1 问题

当前 agent 在解决问题后，经验仅存在于对话历史中，无法跨会话复用。随着对话增多，有价值的模式、踩坑记录和解决方案会逐渐被遗忘，导致模型在相似场景下重复犯错或无法复用已有的正确解法。

现有 `long_term_remember` 工具面向**用户主动**调用，要求用户判断并手工输入内容，使用门槛高、频率低。

### 1.2 目标

新增 **`summarize_experience`** 工具，允许**模型自主判断**当前回复是否包含值得保存的经验，并调用工具将经过提炼的经验文本写入长期记忆（SQLite），供后续会话在相关场景下自动检索。

核心原则：

- 模型驱动（模型判断何时值得保存，而非用户指定）
- 提炼优先（存模型生成的经验摘要，而非原始回复全文）
- 低门槛复用（通过标签和向量检索实现跨会话发现）

---

## 2. 整体设计

### 2.1 数据流

```
┌─────────────────────────────────────────────────────────┐
│  当前会话                                                    │
│                                                          │
│  user: 如何在 Rust 中高效合并两个 Vec？                      │
│  assistant: [回复中演示了 extend + reserve 技巧]              │
│                                                          │
│  模型判断 → "这个 extend+reserve 模式值得保存"              │
│       ↓ 调用 summarize_experience                         │
│       { experience: "用 Vec::extend + reserve 替代        │
│         循环 push，可减少多次扩容拷贝...",                  │
│         tags: ["rust", "performance", "vec"] }           │
│                                                          │
│       → explicit_remember_blocking()                     │
│       → SQLite long_term_memory 表（source_kind=explicit） │
│                                                          │
│  后续会话检索                                               │
│  user: 合并 Vec 很慢怎么办？                               │
│  prepare_messages() → 向量相似度检索                       │
│  → "用 Vec::extend + reserve ..." 被注入上下文              │
└─────────────────────────────────────────────────────────┘
```

### 2.2 与现有工具的关系

| 工具 | 调用者 | 内容来源 | 典型场景 |
|------|--------|----------|----------|
| `long_term_remember` | 用户（显式） | 用户手工输入 | 用户主动标记重要内容 |
| `summarize_experience` | 模型（自主） | 模型从回复中**提炼** | 模型认为当前解法有通用价值 |
| 自动回合索引 | 系统（自动） | user/assistant 原始正文 | 记录完整问答片段 |

> `summarize_experience` 与 `long_term_remember` 共用同一 SQLite 表（`source_kind='explicit'`），区别仅在于调用来源和内容是否经过提炼。

---

## 3. 接口设计

### 3.1 工具规格

**工具名称**: `summarize_experience`

**描述**（给模型的提示）:

> 将本轮对话中提炼出的核心经验写入长期记忆。适用于：解决了一个有价值的问题、发现了一个通用模式、记录了一个重要踩坑。模型应自主判断何时调用，避免滥用。须启用 `long_term_memory_enabled`。

**参数 schema**:

```json
{
  "type": "object",
  "properties": {
    "experience": {
      "type": "string",
      "description": "从本轮对话中提炼的核心经验（由模型生成）。应简洁、通用、可复用。避免复述原问题，聚焦解法和原理。"
    },
    "tags": {
      "type": "array",
      "items": { "type": "string" },
      "description": "经验分类标签，用于后续检索过滤。如语言/框架名（rust、python）、场景（debug、perf、git）、技术主题（async、vec、ownership）。",
      "default": []
    },
    "ttl_secs": {
      "type": "integer",
      "description": "过期秒数；0 或省略表示永不过期（仍受条数上限淘汰）。",
      "default": 0
    }
  },
  "required": ["experience"]
}
```

**返回值**:

- 成功: `"已将经验写入长期记忆 id={id}（tags={:?}）"`
- 失败: `"错误：{具体原因}"`

---

## 4. 存储设计

### 4.1 SQLite 表

复用现有 `crabmate_long_term_memory` 表，无需新增表。

```
INSERT INTO crabmate_long_term_memory
  (scope_id, chunk_text, source_role, created_at_unix,
   expires_at_unix, tags_json, source_kind, embedding)
VALUES
  ('{conversation_id}', '{experience}', 'summarize_experience',
   {now}, {expires_at}, '{tags_json}', 'explicit', {embedding});
```

| 字段 | 含义 |
|------|------|
| `source_role` | 固定为 `"summarize_experience"`（区分用户显式调用） |
| `source_kind` | `"explicit"`（与 `long_term_remember` 一致） |
| `tags_json` | 标签数组的 JSON 序列化 |

### 4.2 向量化

当 `cfg.long_term_memory_vector_backend == Fastembed` 时，存储 passage embedding（与 `long_term_remember` 完全相同）。前缀为 `passage: {experience}`。

---

## 5. 行为约束

### 5.1 内容质量门槛

- `experience` 长度 ≥ 20 字符（过短无保存价值）
- `experience` 长度 ≤ `cfg.long_term_memory_max_chars_per_chunk`（自动截断）
- 写入前检查去重（`has_duplicate_text`），避免重复记录相同经验

### 5.2 调用频率

- 每个 agent 回合最多触发一次（由模型自行控制）
- 不做额外频率限制（依赖模型判断 + system prompt 引导）
- 仍受全局条数上限约束（`cfg.long_term_memory_max_entries`）

### 5.3 System Prompt 引导

在 system prompt 中加入：

```
当解决了一个有通用价值的问题、发现值得复用的模式、或踩过一个有意义的坑时，
主动调用 summarize_experience 将经验存入长期记忆，供后续参考。
经验内容应简洁（1-3 句话），聚焦"怎么做"和"为什么有效"。
```

---

## 6. 实现步骤

### Step 1: 添加工具函数

**文件**: `src/tools/long_term_memory_tools.rs`

新增 `SummarizeExperienceArgs` 结构体和 `summarize_experience()` 函数。内部调用 `rt.explicit_remember_blocking()`，与 `long_term_remember` 共用同一写入路径。

### Step 2: 添加参数构建器

**文件**: `src/tools/tool_params/mod.rs`

新增 `params_summarize_experience()` 函数，返回 `ParamBuilder::Object`。

### Step 3: 注册工具规格

**文件**: `src/tools/tool_specs_registry/specs/diagnostics_docs.inc.rs`

新增 `ToolSpec { name: "summarize_experience", ... }`。

### Step 4: 添加工具标签组

**文件**: `src/tools/dev_tag.rs`

在 `GENERAL` 组或新建 `EXPERIENCE` 组中注册 `"summarize_experience"`。

### Step 5: 暴露 runner 函数

**文件**: `src/tools/mod.rs`（或 `misc_basic.inc.rs` 同一引入层级）

```rust
fn runner_summarize_experience(args: &str, ctx: &ToolContext<'_>) -> String {
    long_term_memory_tools::summarize_experience(args, ctx)
}
```

### Step 6: 更新 System Prompt

**文件**: `config/prompts/default_system_prompt.md`

添加 `summarize_experience` 调用引导段落。

---

## 7. 额外设计考量

### 7.1 调用时机：回合边界问题

`summarize_experience` 应当在一个**完整回复已经产生**之后调用，而非回复中间。这是因为：

- 流式输出场景下，回复尚未完成时，模型无法判断最终内容是否有价值
- 如果模型在回复中途调用，可能只记录了部分内容

**当前设计**：模型在 tool_calls 中调用 `summarize_experience`，其触发时机由模型自行判断（依赖 system prompt 引导）。这意味着模型需要在回复结束后才能发起工具调用——在 `assistant` 消息带有 `tool_calls` 时，`summarize_experience` 本身是另一轮工具调用，需确保该轮次不产生新的经验（避免嵌套循环）。

**隐患**：如果模型在同一轮回复中既输出了最终答案又调用了 `summarize_experience`，工具调用本身可能打断回复的完整性；且该调用发生在当前回复最终确定之前。

**建议**：在 system prompt 中明确引导模型在**最终回复已完整后**才调用该工具，避免在回复中途触发。

### 7.2 经验粒度：单一经验 vs. 多条经验

一个问答轮次可能包含多个独立经验（如同时解决了两个不相关的问题），但当前工具设计为**每次调用写入一条经验**。

**选项 A（当前设计）**：模型判断多个经验时多次调用工具。优点是灵活；缺点的调用次数不确定，模型可能遗漏。

**选项 B**：schema 支持 `experiences: Vec<{experience, tags}>`，单次调用批量写入。优点是减少工具调用次数；缺点是错误处理复杂（部分成功时回滚困难）。

**当前选用选项 A**，单条经验更简单，去重和错误处理独立。若后续发现调用频率过低，再考虑扩展为批量接口。

### 7.3 经验独立性：上下文剥离

模型生成的经验必须**独立于原始对话上下文**即可理解。如果经验内容依赖对话历史中的前置条件（如"上文的那个 API"、"前面说的文件"），后续检索到该经验时将无法理解。

**示例**：

- ❌ 不好："上文中用 `cargo build --release` 编译后，把生成的二进制复制到 `/usr/local/bin` 即可"
- ✅ 好："Rust 项目发布时，用 `cargo build --release` 编译后，将 `target/release/{bin}` 安装到 `PATH` 中即可全局使用"

**缓解**：在工具描述中强调"避免复述原问题，聚焦解法和原理"，并在 system prompt 中引导生成**自包含**的经验文本。

### 7.4 经验的时效性

随着代码库、依赖库或工具版本变化，曾经有效的经验可能**变得无效甚至有害**。

**问题示例**：
- "使用 `serde_json::from_str::<T>(&s)` 时需要 `T: Deserialize`" — 版本稳定，经验长期有效
- "在 v0.4 版本的库中，使用 `X` API 会 panic" — 大版本升级后经验可能有害

**应对策略**：

| 策略 | 说明 |
|------|------|
| TTL 机制 | `ttl_secs` 参数允许设置过期时间；模型在记录踩坑类经验时可设置较短 TTL |
| 标签标注版本上下文 | 鼓励模型在 `tags` 中标注相关版本或库名（如 `["serde_v0.4", "breaking-change"]`） |
| 长期记忆淘汰 | 受 `long_term_memory_max_entries` 限制，最老条目会被自动淘汰 |
| 用户纠错 | 用户可通过 `long_term_forget` 删除过时经验 |

> 当前设计**不主动**处理经验时效性，依赖 TTL 和用户主动清理。未来可扩展增加 `version_context` 字段或版本感知检索。

### 7.5 标签规范控制

模型生成的 `tags` 是自由文本，存在以下风险：

- **噪音标签**：模型可能添加无关标签（如 `["good", "important"]`）
- **不一致同义词**：`rust`、`Rust`、`rust-lang` 同时存在无法合并检索
- **过于具体**：`["2024-03-15-fix", "issue-1234"]` 无通用价值

**缓解措施**：

- 在工具描述中给出**标签示例**（语言名、场景名、技术主题）
- 在 system prompt 中引导使用**统一、可复用**的标签集
- **未来扩展**：可增加标签白名单或标准化层（大小写归一、同义词合并）

### 7.6 对话轮次嵌套：经验的经验

当模型调用 `summarize_experience` 时，该工具调用本身产生的结果（`"已将经验写入..."`）是否**也应该被考虑**为一条潜在经验？

**当前设计**：不处理。`summarize_experience` 的执行结果视为普通工具结果，不会递归触发新一轮经验提炼。

**潜在问题**：如果工具返回 `"错误：experience 不能为空"`，模型可能将其理解为一个反面教材并额外调用一次——但这并非设计意图。

### 7.7 与自动回合索引的关系

`summarize_experience` 写入的经验和**自动回合索引**（`index_turn_blocking`）写入的内容可能高度重复，尤其当模型对每个轮次都调用经验提炼时。

**区别**：

| 维度 | 自动回合索引 | `summarize_experience` |
|------|-------------|------------------------|
| 内容 | user/assistant 原始正文 | 模型**提炼**的摘要 |
| 质量 | 原始、完整、可能冗长 | 提炼、简洁、通用性强 |
| 来源 | 系统自动 | 模型自主判断 |
| 粒度 | 整轮问答 | 单个有价值 insight |

**结论**：两者互为补充。自动索引保留完整上下文，经验提炼提取精华。若模型每次都调用 `summarize_experience`，自动索引的价值相对下降，但两者不会冲突（可同时存在）。

### 7.8 多用户 / 多会话的数据隔离

当前 SQLite 以 `scope_id = conversation_id` 为隔离边界，同一数据库文件支持多会话共存。

**设计考量**：

- **当前**：经验以 `conversation_id` 为 scope，后续检索只返回同会话经验。这限制了跨会话复用的价值。
- **若要跨会话**：需将 `scope_id` 改为用户级或项目级作用域，使经验可在多会话间共享检索。这涉及较大的语义变化，需另行设计。
- **当前设计保持会话级隔离**：符合 `long_term_memory` 的原有语义，简单安全。

### 7.9 检索时的上下文污染

当 `prepare_messages()` 将多条经验同时注入上下文时，如果经验之间存在**矛盾或冲突**（如两个经验给出了相反的建议），模型没有机制识别这种冲突。

**示例**：
- 经验 A（2024年）："使用 `Rc<RefCell<T>>` 管理内部可变性"
- 经验 B（2026年）："已弃用 `RefCell`，建议使用 `Mutex<T>`"

**缓解**：目前无自动解决机制，依赖 TTL 淘汰旧经验和用户手动清理。未来可考虑增加 `supersedes` 字段建立经验间的替代关系。

### 7.10 Token 预算与检索上限

经验提炼本身需要消耗额外 LLM 调用（作为工具调用结果），且每条经验在检索时都会占用上下文窗口。

**当前**：`long_term_memory_inject_max_chars`（默认配置）限制了注入上下文的总量，如果经验条数多或单条长，仍可能超出预算。

**隐忧**：模型可能生成冗长的经验文本（长篇大论），既浪费存储，也浪费检索时的上下文预算。

**缓解**：`experience` 最小长度限制（≥20）和最大长度限制（`max_chars_per_chunk`）提供了硬约束；system prompt 引导"简洁、1-3句话"提供软约束。

### 7.11 经验置信度问题

模型基于一次成功案例生成的经验，可能具有**偶然性或样本偏差**，不一定具有普遍适用性。

**示例**：模型在某次调试中通过"重启 Docker"解决了问题，记录为经验。但实际根因是文件描述符泄漏，重启只是掩盖了问题。

**当前设计**：不做置信度标注，全靠模型自行判断。经验来源标签（`source_role = "summarize_experience"`）可作为未来区分"经验"与"事实"的依据。

**未来可扩展**：
- 增加 `confidence: high/medium/low` 字段
- 增加 `context_dependencies` 字段标注经验适用条件

---

## 8. 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| 模型频繁调用导致记忆噪音 | system prompt 强调"有通用价值"场景；设置长度门槛 |
| 经验内容包含对话上下文（过长、不通用） | 依赖工具描述引导模型提炼；可后续增加"经验不应包含具体项目路径"等规则 |
| 与 `long_term_remember` 重复存储 | 共用去重检查（`has_duplicate_text`）；共用水印前缀 |
| 标签不规范（模型乱标） | 工具描述中给出标签示例；后续可加标签白名单 |
| 经验内容依赖对话上下文，独立不可理解 | 工具描述强调"自包含"；system prompt 引导生成独立经验文本 |
| 过时经验误导未来会话 | TTL 机制 + 用户手动清理；引导踩坑类经验设置短 TTL |
| 模型在回复中途调用，导致经验不完整 | system prompt 引导"回复完整后再调用"；最小长度门槛兜底 |
| 经验之间存在冲突，无自动消解机制 | 当前无自动解决；依赖 TTL 淘汰旧经验；未来可扩展 `supersedes` 字段 |
| 长经验浪费上下文预算 | 最大长度限制（`max_chars_per_chunk`）；system prompt 引导简洁表述 |
| 模型将偶然成功经验化，以偏概全 | 当前无置信度机制；未来可增加 `confidence` 字段 |

---

## 9. 测试计划

| 测试场景 | 预期结果 |
|----------|----------|
| 调用 `summarize_experience`，内容正常 | 返回 `id=N`，SQLite 中有对应行 |
| `experience` 为空 | 返回错误提示 |
| `experience` 长度 < 20 | 返回错误提示 |
| 同一条经验调用两次（去重） | 第二次返回已有 id，不重复插入 |
| 未启用 `long_term_memory_enabled` | 返回错误提示 |
| `tags` 和 `ttl_secs` 均正常传入 | 存储后检索包含正确标签和过期时间 |
