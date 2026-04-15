# `summarize_experience` 待实现功能

本文档记录 `summarize_experience` 工具在当前实现基础上，**尚需进一步实现或扩展**的设计项，按优先级与依赖关系组织。已实现的功能见 [summarize_experience.md](./summarize_experience.md)。

---

## 一、高优先级（影响工具质量，建议近期实现）

### 1.1 标签白名单与标准化

**问题**：`tags` 为自由文本，模型可能生成噪音标签（`"good"`, `"important"`）或同义词不统一（`"rust"` vs `"Rust"` vs `"rust-lang"`）。

**待实现**：
- 在 `AgentConfig` 中增加 `long_term_memory_allowed_tags: Vec<String>` 白名单配置
- `summarize_experience` 执行时对 `tags` 做交集过滤：只保留白名单中的标签（不在白名单的标签忽略并记录日志）
- 标签大小写归一化（统一小写）
- 同义词合并（如 `"ownership"` ← `"own"`, `"rc"`, `"refcell"`）
- 若白名单为空则不限制（向后兼容）

**关联文件**：`src/config/types.rs`、`src/tools/long_term_memory_tools.rs`

---

### 1.2 经验内容自包含校验

**问题**：模型生成的经验可能依赖对话上下文（如"上文中提到的 X"），导致后续检索时无法理解。

**待实现**：
- 在 `summarize_experience` 工具描述中明确规则：经验文本不应包含"上文"、"前面"、"那次"、"上回"等指代词
- 可在函数中增加启发式检查：若经验文本中出现项目绝对路径、个人主机名、具体的 `conversation_id`/`session_id` 模式，发警告日志（不拒绝写入，但不阻塞）
- 未来可扩展为 `experience_clean` 预处理步骤（剥离上下文依赖词）

**关联文件**：`src/tools/long_term_memory_tools.rs`

---

## 二、中优先级（影响工具可靠性和安全性）

### 2.1 置信度 / 经验来源标注

**问题**：模型基于一次成功案例生成的经验可能具有偶然性，不一定具有普遍适用性（如重启 Docker 解决了问题但根因是 fd 泄漏）。

**待实现**：
- 在 schema 中增加可选字段 `confidence: "high" | "medium" | "low"`（默认 `"medium"`）
- 踩坑类经验建议模型标注 `confidence: "low"` 或设置短 TTL
- 在 SQLite 存储层增加 `confidence TEXT` 列
- 检索时可按置信度过滤或排序

**关联文件**：`src/long_term_memory_store.rs`、`src/tools/long_term_memory_tools.rs`、`src/tools/tool_params/diagnostics.rs`

---

### 2.2 经验消解与替代关系

**问题**：当两条经验存在冲突时（如经验 A 推荐 `RefCell`，经验 B 说 `RefCell` 已弃用），模型没有机制识别和消解冲突。

**待实现**：
- 在 SQLite 增加 `supersedes_id INTEGER` 列（指向同一 scope 下另一条经验的主键）
- 当模型通过 `summarize_experience` 写入新经验时，可附带 `supersedes_id` 标注该经验替代了哪条旧经验
- 检索注入时，过滤已被替代的经验（除非显式要求展示冲突历史）
- API：`summarize_experience` 增加可选参数 `supersedes_memory_id: Option<i64>`

**关联文件**：`src/long_term_memory_store.rs`、`src/long_term_memory.rs`

---

### 2.3 版本上下文标注

**问题**：经验可能与特定库版本绑定，版本升级后经验失效。

**待实现**：
- 在 schema 增加可选字段 `version_context: string`（如 `"serde v0.4"`, `"rustc 1.70+"`）
- 鼓励模型在记录涉及版本相关踩坑时标注
- 未来可支持基于当前项目依赖版本过滤经验（与 `Cargo.lock` / `package.json` 比对）

**关联文件**：`src/tools/tool_params/diagnostics.rs`、`src/long_term_memory_store.rs`

---

## 三、低优先级（体验优化，未来按需实现）

### 3.1 批量写入接口

**问题**：一个问答轮次可能包含多个独立经验，但当前每次调用只能写入一条。

**待实现**：
- 新增工具 `summarize_experiences`（复数），接受 `experiences: Vec<{experience, tags, ttl_secs}>` 参数
- 单次工具调用批量写入多条经验，减少工具调用次数
- 错误处理：部分成功时回滚全部或记录失败列表

**关联文件**：`src/tools/long_term_memory_tools.rs`、`src/tools/tool_params/diagnostics.rs`、`src/tools/tool_specs_registry/specs/diagnostics_docs.inc.rs`

---

### 3.2 经验调用频率限制

**问题**：模型可能在每个轮次都调用 `summarize_experience`，导致记忆噪音。

**待实现**：
- 在 `AgentConfig` 增加 `summarize_experience_per_turn_limit: usize`（默认 1）
- 在 `ToolContext` 中增加 `turn_summarize_count: Cell<usize>` 或等效计数器
- `dispatch_tool` 在调用 `summarize_experience` 前检查计数器，超限返回错误而非执行

**关联文件**：`src/tool_registry/execute.rs`、`src/config/types.rs`

---

### 3.3 跨会话经验共享（作用域扩展）

**问题**：当前经验以 `conversation_id` 为 scope，后续检索只返回同会话经验，限制了跨会话复用价值。

**待实现**：
- 在 `AgentConfig` 增加 `long_term_memory_shared_scopes: Vec<String>` 配置
- 支持将经验写入项目级 scope（如 `project:{project_name}`），使同一项目的多个会话共享检索
- 需要新增 `long_term_memory_join_scope` 工具或配置项，允许模型将当前会话经验提升为项目级

**关联文件**：`src/long_term_memory.rs`、`src/tools/long_term_memory_tools.rs`

---

### 3.4 经验检索结果冲突提示

**问题**：`prepare_messages` 同时注入多条经验时，若存在冲突（方向矛盾），模型无法自动识别。

**待实现**：
- 在 `prepare_messages` 注入的经验文本前增加标记：`[经验1/共N条]`、`[注意：以下经验仅供参考，请根据当前上下文判断适用性]`
- 或在注入的数据结构中增加 `conflicts_with: Vec<i64>` 字段，供模型识别冲突经验

**关联文件**：`src/long_term_memory.rs`

---

### 3.5 经验使用效果反馈

**问题**：无法判断存储的经验是否真的在后续被复用、是否仍然有效。

**待实现**：
- 在 SQLite 增加 `hit_count INTEGER DEFAULT 0` 和 `last_hit_unix INTEGER` 字段
- 每次 `prepare_messages` 检索并注入经验后，更新对应行的命中计数和时间戳
- 新增工具 `long_term_memory_stats`：返回各经验的命中频率、最后使用时间等统计信息
- 用户可根据统计信息决定清理低命中经验

**关联文件**：`src/long_term_memory_store.rs`、`src/long_term_memory.rs`

---

### 3.6 模型生成经验的长度与质量指导

**问题**：模型可能生成过长或过短的经验文本，或生成的信息密度不足。

**待实现**：
- 在工具描述中明确：经验长度应在 20-500 字符之间，理想为 80-200 字符
- 在 system prompt 引导中增加"反面案例"示例（过短/过长/包含上下文引用的示例）
- 未来可增加 LLM 自评机制：在写入前让模型对经验质量打分，过低则拒绝写入

---

## 四、已明确不在当前版本实现（拒绝项）

以下条目在当前设计中**明确不实现**，仅作记录：

| 条目 | 原因 |
|------|------|
| 自动从自动索引经验中提炼（无需模型调用） | 违背"模型自主判断"的核心设计原则；自动提炼质量不可控 |
| 经验全文加密或访问控制 | 超出当前 SQLite 存储层设计范围；按会话隔离已满足基本安全需求 |
| 跨模型经验迁移（从 A 模型经验迁移到 B） | 依赖模型能力差异，经验格式不通用，实现成本高 |
