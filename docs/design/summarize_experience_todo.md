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

## 四、经验层：从存储到知识体系

以下方向在 `summarize_experience` 基础上，将记忆能力从"孤立点存储"升级为"有机关联的知识体系"。

### 4.1 经验链（Experience Chain）

**问题**：单条经验是孤立的，但真实认知往往是链式的——"踩坑 A → 尝试 B 失败 → 尝试 C 成功 → 提炼经验 E"。孤立存储丢失了过程上下文。

**设计方向**：
- 在 SQLite 增加 `chain_id INTEGER` 和 `chain_order INTEGER` 字段
- 模型在记录经验时，可选择创建新链（`chain_id`）或追加到已有链（通过 `summarize_experience` 传入 `chain_id`）
- 每条经验带 `role_in_chain: "problem" | "failed_attempt" | "success" | "insight"` 标记角色
- 检索注入时，同一 chain 的经验可选择性展开（全部展开 / 仅头尾 / 仅最终 insight）
- 新增工具 `start_experience_chain(experience, problem_description)` 创建链；`append_experience(chain_id, experience, role)` 追加节点

**关联文件**：`src/long_term_memory_store.rs`、`src/long_term_memory.rs`、`src/tools/long_term_memory_tools.rs`

---

### 4.2 经验自检（Experience Self-Review）

**问题**：经验写入后即被遗忘。随着时间推移，经验可能过时、与新方案冲突、或已被更好的解法替代，却无人清理。

**设计方向**：
- 新增工具 `review_experiences`，在每个会话开始、`prepare_messages` 注入记忆后触发
- 模型对比当前对话上下文，判断已注入的每条经验是否：**仍然有效 / 已过时 / 与当前方案冲突**
- 对过时或冲突的经验，返回修改建议：`[{"id": 42, "action": "shorten_ttl", "reason": "serde v0.4 deprecated the API"}, {"id": 87, "action": "supersede", "supersedes_id": 91}]`
- 系统根据建议自动执行（缩短 TTL、写入替代关系）或交由用户确认
- 可在 `AgentConfig` 中配置 `auto_review_on_session_start: bool`（默认开启）

**关联文件**：`src/long_term_memory.rs`、`src/tools/long_term_memory_tools.rs`

---

### 4.3 置信度传播（Confidence-propagated Memory）

**问题**：低置信度经验（如一次偶然成功的排查）与高置信度经验（如广泛验证的通用模式）平等占用 token 预算，可能误导模型。

**设计方向**：
- `SummarizeExperienceArgs` 增加 `confidence: "high" | "medium" | "low"`（默认 `"medium"`）
- 踩坑类、首次解决类经验引导模型设置 `confidence: "low"`；有文档/多案例验证的设 `"high"`
- 在 SQLite 增加 `confidence TEXT` 列
- `prepare_messages` 注入时按置信度排序：high 经验优先、full budget；low 经验只给一句话摘要或低于 budget 阈值时跳过
- 低置信度经验默认设置短 TTL，自动淘汰

**关联文件**：`src/long_term_memory_store.rs`、`src/long_term_memory.rs`、`src/tools/long_term_memory_tools.rs`、`src/tools/tool_params/diagnostics.rs`

---

## 五、交互层：更智能的上下文感知

### 5.1 工作区风格记忆（Workspace Style Memory）

**问题**：当前经验以 `conversation_id` 为 scope，换一个会话经验就丢失。但一个项目特有的风格规范（如本仓库偏好 `cargo clippy -D warnings` 而非仅 `cargo check`）是跨会话通用的。

**设计方向**：
- 新增 `workspace_style_remember(experience, style_type)` 工具，写入 scope 为 `workspace:{workspace_root_hash}` 的经验池
- `style_type` 如 `"formatting" | "git_convention" | "tool_preference" | "naming"` 用于分类
- 在 `prepare_messages` 中，工作区改变时自动检索对应 style memory 并注入
- 用户可通过 `workspace_style_list` 查看当前工作区的风格规范
- 与普通经验分离：workspace style 永不自动淘汰（除非显式删除）

**关联文件**：`src/long_term_memory.rs`、`src/tools/long_term_memory_tools.rs`

---

### 5.2 主动经验提示（Proactive Experience Hints）

**问题**：当前经验只在 `prepare_messages` 时被动注入。模型需要主动回忆，而非被动等待。

**设计方向**：
- 在 `dispatch_tool` 执行工具后，比对工具名/参数与高置信度经验池
- 若检测到强相关经验（如正在 `cargo build` 而经验池里有"大项目增量编译加速技巧"），在工具返回结果后附加非侵入式提示
- 提示格式：`[💡 相关经验：{short_summary}]`，不打断工具结果流
- 通过 `AgentConfig` 配置 `proactive_hint_enabled: bool` 和 `proactive_hint_max_per_turn: usize`
- 注意：此功能为"附加建议"，不影响工具执行结果本身

**关联文件**：`src/tool_registry/execute.rs`、`src/long_term_memory.rs`

---

### 5.3 失败模式库（Anti-Pattern Memory）

**问题**：`summarize_experience` 只记录成功经验，但**失败教训往往比成功经验更有价值**——"此路不通"的信息密度极高。

**设计方向**：
- 新增独立工具 `remember_mistake`，接受 `anti_pattern: String, why_failed: String, attempts_made: Vec<String>, tags`
- 标签中自动附加 `["anti-pattern"]` 水印
- TTL 默认更短（建议 7 天），因为失败模式可能随技术演进失效更快
- 检索时单独注入：`[⚠️ 反模式]` 标记，与普通经验区分显示
- 模型在规划阶段（plan + execute 之前）可主动查询 anti-pattern 作为排除列表
- 与 `summarize_experience` 共用 `explicit_remember_blocking` 写入路径，共用 embedding 检索，存储层完全复用

**关联文件**：`src/tools/long_term_memory_tools.rs`、`src/tools/tool_params/diagnostics.rs`

---

## 六、系统层：工具体系的智能化

### 6.1 工具使用成功率追踪（Tool Hit-Rate Tracker）

**问题**：模型调用工具后，无法判断这个工具**是否真的解决了问题**。长期积累后可发现工具设计盲点。

**设计方向**：
- 在 `dispatch_tool` 返回后，轻量记录 `tool_name + session_id + turn_id + timestamp + execution_time_ms + outcome`
- `outcome` 为 `ToolOutcome { helped: bool, confidence: high/medium/low, notes: Option<String> }`
- 模型在每轮结束后可收到一个非侵入式的"本次工具效果反馈"提示（可选）
- 新增工具 `get_tool_stats(tool_name)` 返回该工具的历史帮助率、平均执行时间、失败模式聚合
- 数据存在独立 SQLite 表（与 long_term_memory 表分离）
- 可用于：动态调整工具描述中的 `ToolSummaryKind` 权重、向用户推荐高频高置信度工具

**关联文件**：`src/tool_registry/execute.rs`、新增 `src/tool_stats.rs`

---

### 6.2 动态工具推荐（Dynamic Tool Suggestion）

**问题**：工具注册表是静态的，模型只能从已有工具中选择，无法根据当前问题场景动态获得工具使用顺序建议。

**设计方向**：
- 在 `AgentConfig` 中增加 `dynamic_tool_suggestion_enabled: bool`（默认 false）
- 开启后，在每次 `prepare_messages` 注入时，额外附加"当前上下文推荐工具序列"（基于 embedding 相似度从历史成功案例中检索）
- 不注册新工具，而是对已有工具做**重排序/加权提示**：`[推荐工具顺序：cargo clippy → cargo test → search_in_files]`
- 依赖 `ToolHitRateTracker` 积累的成功率数据
- 轻量实现：不走 LLM，用规则 + 历史命中率排序即可

**关联文件**：`src/long_term_memory.rs`、`src/tool_registry/execute.rs`、`src/config/types.rs`

---

## 七、知识管理：跨会话与跨 Agent

### 7.1 项目经验摘要（Project Memory Digest）

**问题**：经验积累多了，单条经验太细碎，模型在新会话中无法快速建立对项目经验的全貌认知。

**设计方向**：
- 新增 cron 或手动触发的工具 `generate_project_memory_digest`
- 扫描 workspace 级经验池（scope = `workspace:{hash}`），按主题聚类
- 调用 LLM 提炼出"项目踩坑百科"：按主题分类，每类 3-5 条精华，每条 1-3 句话
- 输出格式为 Markdown，存入 `config/project_memory_digest.md`
- 新会话的 `prepare_messages` 自动注入该摘要（作为 system prompt 的一部分，而非普通工具注入）
- 用户可随时 `regenerate_project_digest` 手动刷新

**关联文件**：`src/long_term_memory.rs`、`src/tools/long_term_memory_tools.rs`

---

### 7.2 经验版本化（Experience Versioning）

**问题**：库升级或架构重构后，经验从"正确"变"错误"，但 `forget` 只能删，无法保留历史轨迹。

**设计方向**：
- 与 2.2 "经验消解与替代关系"合并设计：`supersedes_id` 字段实现经验版本链
- 新增工具 `deprecate_experience(id, reason, new_experience_id?)`，将旧经验标记为废弃（设置 `deprecated_at_unix` 字段）
- 废弃经验在检索时默认不展示，但可通过 `long_term_memory_list(include_deprecated: true)` 查看历史
- 自动触发：当 `prepare_messages` 检索时发现已注入经验与当前工具结果矛盾（如执行 `cargo build` 失败，提示旧经验可能过时），主动提示模型调用 `deprecate_experience`

**关联文件**：`src/long_term_memory_store.rs`、`src/long_term_memory.rs`

---

## 八、已明确不在当前版本实现（拒绝项）

以下条目在当前设计中**明确不实现**，仅作记录：

| 条目 | 原因 |
|------|------|
| 自动从自动索引经验中提炼（无需模型调用） | 违背"模型自主判断"的核心设计原则；自动提炼质量不可控 |
| 经验全文加密或访问控制 | 超出当前 SQLite 存储层设计范围；按会话隔离已满足基本安全需求 |
| 跨模型经验迁移（从 A 模型经验迁移到 B） | 依赖模型能力差异，经验格式不通用，实现成本高 |
