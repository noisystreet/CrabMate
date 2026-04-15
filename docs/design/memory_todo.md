# 设计文档：长期记忆可视化面板

## 1. 背景与目标

`summarize_experience` 工具已实现模型自主写入经验的功能，但**用户无法查看模型保存了哪些记忆**。当前只有通过 `long_term_memory_list` 工具在对话中查询，以原始文本形式返回，缺乏结构化展示。

本功能为长期记忆提供**独立可视化面板**，让用户能够：

- 查看当前会话积累的所有记忆条目
- 按标签、时间、来源筛选
- 主动删除不需要的记忆
- 了解每条记忆的命中统计（未来扩展）

---

## 2. 整体架构

```
┌──────────────────────────────────────────────────────────┐
│  前端 Leptos                                               │
│                                                             │
│  ┌─────────────┐    ┌──────────────────────────────────┐  │
│  │ 侧栏图标    │───▶│ MemoryModal（记忆面板）            │  │
│  │ 💾 记忆     │    │                                  │  │
│  └─────────────┘    │  • 列表（id / 内容摘要 / 标签）    │  │
│                     │  • 标签筛选                        │  │
│                     │  • 搜索                           │  │
│                     │  • 删除                            │  │
│                     │  • 来源过滤（experience / remember） │  │
│                     └──────────────────────────────────┘  │
│                              │                              │
│                     GET /memory/list (JSON)               │
└──────────────────────────────│──────────────────────────────┘
                               │
┌──────────────────────────────│──────────────────────────────┐
│  后端 Axum                    ▼                              │
│  routes/memory/mod.rs                                         │
│    GET /memory/list  →  list_recent_blocking()              │
│    DELETE /memory/:id → explicit_forget_blocking()          │
│                                                             │
│  http_types/memory.rs                                        │
│    MemoryListResponse / MemoryDeleteResponse                 │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. API 设计

### 3.1 `GET /memory/list`

返回当前会话（`scope_id = conversation_id`）的未过期记忆条目列表。

**Query 参数**：

| 参数 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `limit` | usize | 64 | 最大返回条数 |
| `source_kind` | string | 空（不过滤） | 过滤来源：`auto` / `explicit` / `summarize_experience` |
| `tag` | string | 空（不过滤） | 按标签精确过滤 |
| `search` | string | 空（不过滤） | 在正文和标签中模糊搜索 |

**响应** `MemoryListResponse`：

```json
{
  "entries": [
    {
      "id": 42,
      "chunk_text": "用 Vec::extend + reserve 替代循环 push，可减少多次扩容拷贝...",
      "source_kind": "summarize_experience",
      "source_role": "summarize_experience",
      "tags": ["rust", "performance", "vec"],
      "created_at_unix": 1744567890,
      "expires_at_unix": null,
      "confidence": "medium",
      "hit_count": 3,
      "supersedes_id": null
    }
  ],
  "total": 1
}
```

### 3.2 `DELETE /memory/:id`

删除指定 id 的记忆条目。

**响应** `MemoryDeleteResponse`：

```json
{
  "deleted_id": 42,
  "ok": true
}
```

**错误**：`404` 当 id 不存在或不属于当前 scope。

### 3.3 `GET /memory/stats`（未来扩展）

返回当前会话记忆统计：总数、按来源分布、命中率分布、平均 TTL 等。

---

## 4. 存储层改动

现有 `long_term_memory_store::list_recent_for_scope` 已返回所需字段，无需改表结构。

后续扩展（见待办）如 `hit_count`、`confidence`、`supersedes_id` 等字段，需要 schema 迁移，届时新增 endpoint 前先完成迁移。

---

## 5. 前端组件设计

### 5.1 入口：侧栏图标按钮

在侧栏导航区（`app/sidebar_nav.rs`）增加一个"记忆"图标按钮：

- 图标：💾 或 Brain icon（Unicode 或 SVG）
- 点击打开 `MemoryModal`
- 有记忆时显示徽标数字（未读新记忆条数，可选）

### 5.2 MemoryModal 组件

新建 `frontend-leptos/src/app/memory_modal.rs`：

```
┌─────────────────────────────────────────────────────────┐
│  记忆面板                              [标签▾] [来源▾]  ✕  │
├─────────────────────────────────────────────────────────┤
│  🔍 搜索记忆…                                           │
├─────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────┐  │
│  │ 经验 #42  ·  summarize_experience  ·  3天前       │  │
│  │ rust  performance  vec                            │  │
│  │ 用 Vec::extend + reserve 替代循环 push，可减少多…  │  │
│  │                          已命中 3 次  [删除]       │  │
│  └───────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────┐  │
│  │ 记忆 #87  ·  long_term_remember  ·  1小时前      │  │
│  │ [rust] [debug]                                   │  │
│  │ 别名冲突时优先使用模块前缀而非 use as 重命名       │  │
│  │                                     已命中 1 次   │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

**功能**：

- **列表展示**：每条记忆显示 id、来源标签、内容摘要（200字符截断）、标签、创建时间、命中次数（未来）
- **标签筛选**：下拉多选，按标签过滤
- **来源筛选**：下拉选项：`全部` / `模型提炼` / `用户显式` / `自动索引`
- **全文搜索**：在内容和标签中模糊搜索
- **删除**：每条记忆行有删除按钮，点击确认后删除

### 5.3 Signal / State

在 `ChatSessionSignals` 或新建 `MemorySignals` 中管理：

```rust
pub struct MemorySignals {
    pub modal_open: RwSignal<bool>,
    pub entries: RwSignal<Vec<MemoryEntry>>,
    pub loading: RwSignal<bool>,
    pub filter_tag: RwSignal<Option<String>>,
    pub filter_source: RwSignal<Option<String>>,
    pub search_query: RwSignal<String>,
}
```

### 5.4 i18n 文本

在 `i18n/` 下新建 `memory.rs`，提供：

```rust
pub fn memory_title(l: Locale) -> &'static str
pub fn memory_empty(l: Locale) -> &'static str
pub fn memory_delete_confirm(l: Locale) -> &'static str
pub fn memory_filter_all(l: Locale) -> &'static str
pub fn memory_filter_summarize(l: Locale) -> &'static str
pub fn memory_filter_explicit(l: Locale) -> &'static str
pub fn memory_filter_auto(l: Locale) -> &'static str
pub fn memory_source_summarize(l: Locale) -> &'static str
pub fn memory_source_explicit(l: Locale) -> &'static str
pub fn memory_source_auto(l: Locale) -> &'static str
pub fn memory_hit_count(l: Locale) -> &'static str
pub fn memory_delete(l: Locale) -> &'static str
pub fn memory_nav_aria(l: Locale) -> &'static str
```

---

## 6. 实现步骤

### Step 1: 后端 API

**新增文件**：

- `src/web/routes/memory/mod.rs` — 路由注册
- `src/web/http_types/memory.rs` — 请求/响应类型
- `src/web/memory/handlers.rs` — handler 实现

**改动文件**：

- `src/web/routes/mod.rs` — 注册 `/memory` 路由
- `src/web/app_state.rs` — `long_term_memory` 已在 state 中，无需新增

**Handler 实现**：

```rust
// GET /memory/list
async fn memory_list_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<MemoryListQuery>,
) -> Json<MemoryListResponse>

// DELETE /memory/:id
async fn memory_delete_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<MemoryDeleteResponse>, StatusCode>
```

### Step 2: 前端 API 客户端

**新增文件**：`frontend-leptos/src/api/memory.rs`

```rust
pub async fn list_memories(query: MemoryListQuery) -> Result<MemoryListResponse, ApiError>
pub async fn delete_memory(id: i64) -> Result<MemoryDeleteResponse, ApiError>
```

### Step 3: 前端 Signal/State

**新增文件**：`frontend-leptos/src/chat_memory_state.rs`

实现 `MemorySignals` 结构体，管理 modal 状态和数据获取。

### Step 4: 前端 MemoryModal 组件

**新增文件**：`frontend-leptos/src/app/memory_modal.rs`

实现 `MemoryModal` 组件，包含列表、筛选、搜索、删除功能。

### Step 5: i18n

**新增文件**：`frontend-leptos/src/i18n/memory.rs`

在 `i18n/mod.rs` 中引入并导出。

### Step 6: 入口绑定

在 `frontend-leptos/src/app/sidebar_nav.rs` 中增加记忆按钮，点击时打开 modal。

---

## 7. 安全与隔离

- `/memory/list` 和 `/memory/delete` 必须校验当前会话的 `conversation_id`（从 session cookie 或 auth token 获取），确保只能访问同会话记忆
- 删除时校验 scope：只能删除属于当前会话的记忆
- 与 `long_term_memory_list` 工具共用同一 SQLite 查询路径，共用权限模型

---

## 8. 待扩展方向

以下功能在基础版实现后按需扩展：

| 功能 | 说明 |
|------|------|
| 命中率统计注入 | 显示"该记忆在 N 个后续会话中被检索到" |
| 记忆编辑 | 修改已存储记忆的标签或 TTL |
| 记忆导出 | 导出为 Markdown / JSON |
| 批量删除 | 按标签、来源或时间范围批量删除 |
| 记忆链视图 | 若实现经验链（Experience Chain），在面板中展示链状关系 |
| 记忆详情弹窗 | 点击展开完整内容（非截断） |

---

## 9. 记忆质量优化

### 9.1 记忆语义压缩（Semantic Memory Compression）

**问题**：多条经验高度相似时，各自独立存储导致噪音和注入预算浪费。

**设计**：当模型写入新经验时，或在定期后台任务中，对经验池做向量相似度聚类：

- 对相似度 > 阈值（如 0.85）的经验 A、B，合并为更通用的一条
- 合并时保留所有标签集的并集，记录被合并经验的 `supersedes_id` 链
- 合并后的经验注明来源：`"由 N 条相似经验合并，原始 id：..."`

**关联**：`src/long_term_memory.rs`

---

### 9.2 记忆新鲜度评分（Memory Freshness Score）

**问题**：TTL 是粗粒度的，经验可能因代码库变化而失效，却未过期。

**设计**：在 `prepare_messages` 检索注入时，对每条经验做 freshness scoring：

- 对经验中涉及的**文件路径**，检查 `git log` 自经验创建以来的变更次数
- 对经验中涉及的**依赖版本**（如 `"serde v0.4"`），与当前 `Cargo.lock` 比对
- freshness score 高的经验优先 full budget 注入，低的标注 `[可能过时]` 或降权

**关联**：`src/long_term_memory.rs`

---

### 9.3 记忆置信度自验证（Confidence Self-Verification）

**问题**：模型声称 `confidence: "high"` 没有验证机制，偶然成功的经验可能长期占用高权重。

**设计**：静默追踪经验实际帮助效果：

- 经验被 `prepare_messages` 注入后，跟踪该会话的 `dispatch_tool` 调用序列
- 若后续工具调用与经验建议一致 → 置信度强化（`hit_count` +1）
- 若模型最终放弃经验建议（从日志分析） → 置信度降级标记
- 数据积累后形成 `经验 → 实际帮助率` 反馈闭环，用于调整注入权重

**关联**：`src/long_term_memory.rs`、`src/long_term_memory_store.rs`

---

## 10. 记忆检索增强

### 10.1 上下文锚定检索（Context-Anchored Retrieval）

**问题**：当前仅用用户 query embedding 检索，用户问题与记忆可能无字面相似度却有深层因果关联。

**设计**：在 `prepare_messages` 中，不仅用用户 query，还用**工具执行结果**作为额外检索信号：

```
用户：cargo build 失败
经验池无字面相似

但工具结果中出现候选项 A 使用 X API，候选项 B 使用 Y API
经验池有："X API 在 v0.3 后有 breaking change"
       ↓ 检索锚点：工具执行结果
       → 该经验被关联并注入
```

**关联**：`src/long_term_memory.rs`

---

### 10.2 跨符号记忆链接（Cross-Symbol Memory Linking）

**问题**：经验涉及特定符号（如函数名）时，无法在模型接触该符号时主动触发关联。

**设计**：在写入经验时，若涉及特定符号，自动建立 `symbol → memory_ids` 反向索引：

- 解析经验文本中的代码符号（函数调用、模块路径等）
- 写入 `memory_symbol_index(scope_id, symbol, memory_id)` 表
- 当模型通过 `rust_analyzer_goto_definition` 等工具接触同一符号时，主动触发相关记忆注入

**关联**：`src/long_term_memory_store.rs`、`src/long_term_memory.rs`

---

### 10.3 主动冲突暴露（Proactive Conflict Exposure）

**问题**：多条相关经验同时注入时，模型无法感知它们之间存在矛盾。

**设计**：在注入时做冲突检测：

```
检测到：经验 A 推荐方案 X
       经验 B 推荐方案 Y，Y 与 X 矛盾

注入信息：
  [经验1/共N条] ...方案X...
  [⚠️ 注意：经验2 与经验1 存在方向矛盾，
   建议优先采纳置信度更高的经验，
   或通过诊断工具验证当前场景适用哪个方案]
```

让模型**知道**存在冲突，而非无意识选择。

**关联**：`src/long_term_memory.rs`

---

## 11. 记忆来源扩展

### 11.1 决策轨迹记忆（Decision Trail Memory）

**问题**：`summarize_experience` 只记录结论，丢失了推导过程。后来者不知道"为什么选 X 而非 Y"。

**设计**：在 `SummarizeExperienceArgs` 增加可选字段 `decision_trail`：

```json
{
  "experience": "处理并发写入冲突时，应优先使用 channel 而非锁",
  "decision_trail": [
    {"attempt": "Mutex", "result": "死锁风险高，放弃"},
    {"attempt": "RWLock", "result": "写并发时仍有竞争，放弃"},
    {"attempt": "mpsc channel", "result": "数据流清晰，成功"}
  ],
  "tags": ["rust", "concurrency"]
}
```

检索时可展开完整决策路径，让模型学到"为什么这样选"而不仅是"用什么"。

**关联**：`src/long_term_memory_store.rs`、`src/tools/long_term_memory_tools.rs`

---

### 11.2 失败归因记忆（Failure Attribution Memory）

**问题**：模型在工具执行失败后调整策略并最终成功的过程，目前没有记录。

**设计**：新增独立工具 `remember_mistake`，专门记录"此路不通"的反模式：

```json
{
  "anti_pattern": "在热路径中使用 serde_json::from_str",
  "why_failed": "每次调用都重新解析，CPU 占用高",
  "attempts_made": ["serde_json", "ron", "custom parser"],
  "correct_alternative": "使用 rkyv 或 postcard 做零拷贝序列化",
  "tags": ["rust", "performance", "serialization"]
}
```

与 `summarize_experience` 对称：一个是成功经验，一个是失败教训。检索时单独注入，带 `[⚠️ 反模式]` 标记。

**关联**：`src/tools/long_term_memory_tools.rs`

---

### 11.3 工作区演化感知（Workspace Evolution Awareness）

**问题**：代码库演进时，记忆中的某些内容会随工作区变化而失效。

**设计**：在 `prepare_messages` 中或定时任务里，扫描工作区变更摘要：

- 新增了哪些模块 / API？
- 哪些文件被高频修改（可能意味着不稳定区域）？
- 经验中涉及的文件自创建以来变更次数？
- 基于变更自动给已有记忆打上"可能过时"标记或触发自检提醒

**关联**：`src/long_term_memory.rs`

---

## 12. 记忆组织结构

### 12.1 记忆集合 / Curated Collections

**问题**：记忆是扁平的，用户无法主动组织相关记忆。

**设计**：用户能够创建命名集合，将相关记忆归类：

```
收藏集：
  - "Rust 性能模式"（5条）
      - #12: Vec::extend vs loop push
      - #34: 避免在 hot path 用 serde_json
      - ...
  - "Git 急救手册"（3条）
  - "本项目特定规范"（2条）
```

新增 `memory_collections` 表和 `collection_members` 表，提供创建/添加/删除集合的 API。集合可导出、分享。

**关联**：`src/long_term_memory_store.rs`

---

### 12.2 记忆叙事生成（Memory Narrative Generation）

**问题**：用户逐条查看记忆效率低，无法快速建立全貌认知。

**设计**：新增工具 `generate_memory_digest`，基于当前记忆池生成叙事文本：

```
## 性能优化
您在 crabmate 中多次遇到 Rust 编译慢的问题，主要通过以下方式解决：
1. [Rust] 使用 cargo-nextest 而非 cargo test（平均提速 3x）
2. [Rust] 大模块开启增量编译缓存
3. [依赖] 避免在 hot path 中引入 serde_json 的序列化开销

## Git 规范
...
```

生成后存入 `config/memory_digest.md`，新会话自动注入作为 system prompt 补充。

**关联**：`src/tools/long_term_memory_tools.rs`

---

### 12.3 记忆关联图谱（Memory Knowledge Graph）

**问题**：扁平记忆列表无法展示记忆之间的语义关联。

**设计**：以符号（函数/模块/文件）和 tag 为节点，记忆为边，构建轻量知识图谱：

```
rust_analyzer_goto_definition ──关联──▶ [rust, "符号跳转依赖rust-analyzer可用性"]
      │
      └─冲突──▶ [rust, "workspace 不干净时 rust-analyzer 行为异常"]
```

前端面板中以可折叠图谱形式展示记忆网络，帮助用户理解记忆间的关联与冲突。

**关联**：`src/long_term_memory_store.rs`（新增图谱索引表）、前端组件

---

## 13. 记忆系统自学习

### 13.1 记忆使用元数据追踪（Memory Meta-Learning）

**问题**：无法评估 `summarize_experience` 工具本身的使用效率。

**设计**：记录工具使用模式本身：

- 模型在什么场景下倾向调用 `summarize_experience`？
- 哪些场景触发后**从未**被记录为经验？（发现模型低估经验价值的盲点）
- 平均每次会话调用几次？频率是否与问题复杂度正相关？

这些元数据帮助调整 system prompt 引导策略或发现工具描述的盲点。数据存入独立 SQLite 表，不污染经验池。

**关联**：`src/tool_stats.rs`（扩展）

---

### 13.2 经验价值估算（Experience Value Estimation）

**问题**：记忆占 token 预算，注入的记忆越多推理成本越高，但所有经验平等占用预算。

**设计**：为每条经验估算"token 成本 / 预期帮助价值"比：

```
价值分 = 历史命中次数 × 命中后工具成功率提升
成本分 = 经验文本长度 × 平均注入频率
价值/成本比 = value分 / cost分
```

比值高的经验 full budget 注入，比值低的降权或跳过。将价值分作为排序权重注入 `prepare_messages`。

**关联**：`src/long_term_memory.rs`

---

### 13.3 对话式记忆管理（Conversational Memory Management）

**问题**：用户需要打开额外面板才能管理记忆，割裂了对话流。

**设计**：扩展 `long_term_memory_list` 工具，使其在对话中以结构化形式展示（而非纯文本），并支持对话内直接操作：

```
用户：本会话积累了哪些记忆？
助手：当前会话共记录了 3 条记忆：

  1. [rust] Vec::extend 替代循环 push
     标签：rust, performance
     创建于：3天前 | 已命中 3 次
     [删除] [查看详情]

  2. [git] force push 前加 --force-with-lease 更安全
     标签：git, safety
     创建于：1小时前
     [删除] [查看详情]

  3. [debug] 使用 cargo test -- --nocapture 查看 println
     标签：rust, debugging
     创建于：2天前
     [删除] [查看详情]

用户：第二条删了吧
助手：[调用 long_term_forget 删除记忆 #87]
  已删除。当前会话还有 2 条记忆。
```

与 Web UI 记忆面板互为补充：面板适合批量管理，对话内操作适合快速单条处理。

**关联**：`src/tools/long_term_memory_tools.rs`

---

## 14. 导入 / 导出 / 修改

### 14.1 导出（memory_export）

**问题**：当前无法将记忆批量导出，记忆完全被困在 SQLite 中。

**设计**：新增工具 `memory_export`，将当前会话记忆导出为 JSON 或 Markdown：

```json
{
  "format": "json",       // 或 "markdown"
  "source_kind": "all",    // "summarize_experience" | "explicit" | "auto"
  "tags": [],
  "include_embedding": false
}
```

**JSON 导出格式**：

```json
{
  "exported_at_unix": 1744567890,
  "scope_id": "conv_abc123",
  "embedding_model": "AllMiniLML6V2",
  "format_version": "1",
  "entries": [
    {
      "id": 42,
      "chunk_text": "用 Vec::extend + reserve 替代循环 push...",
      "source_kind": "summarize_experience",
      "tags": ["rust", "performance"],
      "created_at_unix": 1744500000,
      "expires_at_unix": null,
      "embedding": "base64-encoded-f32-bytes..."
    }
  ]
}
```

> **关于 embedding**：Embedding 与 embedding 模型强绑定。`AllMiniLML6V2` 导出的 embedding 无法用于 `BAAI/bge-large`。`include_embedding: false` 时导出不包含 embedding，导入时由目标机器的 Fastembed 重新计算。`embedding_model` 字段记录来源，导入时可校验一致性。

**Markdown 导出格式**：人类可读，每条记忆含元信息（来源、标签、TTL）和正文，不含 embedding。

**关联**：`src/long_term_memory_store.rs`、`src/long_term_memory.rs`、`src/tools/long_term_memory_tools.rs`

---

### 14.2 导入（memory_import）

**问题**：无法从外部导入记忆到当前会话。

**设计**：新增工具 `memory_import`，从 `memory_export` 导出的 JSON 批量导入：

```json
{
  "json_content": "{ ... exported json ... }",
  "target_scope": "current_session",
  "skip_on_embedding_mismatch": true,
  "deduplicate_by_text": true
}
```

**返回**：

```json
{
  "imported": 5,
  "skipped": 2,
  "errors": [
    {"id": 42, "reason": "embedding model mismatch and skip_on_embedding_mismatch=false"},
    {"id": 87, "reason": "text duplicate of existing entry #12"}
  ],
  "new_ids": [100, 101, 102, 103, 104]
}
```

**导入行为细则**：

| 情况 | 行为 |
|------|------|
| embedding 模型匹配 | 保留原 embedding，直接插入 |
| embedding 模型不匹配，`skip_on_embedding_mismatch=true` | 跳过 embedding，导入后重新计算 |
| embedding 模型不匹配，`skip_on_embedding_mismatch=false` | 整条跳过 |
| 正文与现有记忆重复，`deduplicate_by_text=true` | 跳过，不报错 |
| `id` 字段 | 忽略，由目标 SQLite 重新分配新 id |
| `created_at_unix` | 保留原时间戳 |
| `expires_at_unix` | 相对 TTL 保留；若原为永不过期则保持 null |

**关联**：`src/long_term_memory_store.rs`、`src/long_term_memory.rs`、`src/tools/long_term_memory_tools.rs`

---

### 14.3 修改（memory_update）

**问题**：已有记忆的正文、标签、TTL 无法修改，只能删除后重建。

**设计**：新增工具 `memory_update`，按 id 或正文匹配修改已有记忆：

```json
{
  "memory_id": 42,
  "memory_text": null,
  "new_text": "新经验文本（可选）",
  "new_tags": null,
  "add_tags": ["rust", "performance"],
  "new_ttl_secs": null
}
```

**返回**：

- 成功：`"已更新记忆 id=42，新标签=['rust', 'performance']，新TTL=不过期"`
- 失败：`"错误：未找到 id=42 的记忆"`

**存储层改动**：新增 `update_chunk()` 函数，更新后触发 embedding 重新计算（因为正文变了）。

**关联**：`src/long_term_memory_store.rs`、`src/long_term_memory.rs`、`src/tools/long_term_memory_tools.rs`

---

### 14.4 批量操作与导出→编辑→导入循环

**批量删除**：扩展 `long_term_forget`，支持 `memory_ids: Vec<i64>` 一次删除多条。

**批量修改标签**：新增 `memory_batch_update_tags(memory_ids, new_tags, mode)`，支持替换或追加模式。

**导出→编辑→导入循环**：导入时按 `id` 做 upsert（id 存在则更新，不存在则插入），用户可以：

1. 导出 JSON
2. 手动编辑 JSON（修改标签、合并重复项、删除不需要的）
3. 导入编辑后的 JSON，回写到同一或不同会话

**关联**：`src/tools/long_term_memory_tools.rs`

---

### 14.5 Web UI 集成

在记忆可视化面板（`MemoryModal`）中增加按钮：

| 按钮 | 位置 | 行为 |
|------|------|------|
| 导出 | 面板顶部 | 弹出格式选择（JSON / Markdown），点击后下载文件 |
| 导入 | 面板顶部 | 弹出文件选择器，上传 JSON 后预览并确认导入 |
| 编辑 | 每条记忆行 | 点击后在行内切换为编辑态，保存触发 `memory_update` |
| 批量选择 | 列表左侧 | checkbox 选择多条后，批量删除 / 批量添加标签 |

---

### 14.6 安全考量

| 考量点 | 缓解措施 |
|------|------|------|
| 导入恶意 JSON（超大体积、畸形数据） | 校验 JSON 结构；限制单次导入条数上限（如 1000 条）；限制 `chunk_text` 单条最大长度 |
| 导入覆盖当前会话重要记忆 | 导入前显示预览（条数、标签摘要）；`deduplicate_by_text` 避免重复 |
| 导出隐私 | 导出前提示用户检查内容；Markdown 导出不含 embedding，无法反向推出文件内容 |
| embedding 模型不一致 | `embedding_model` 字段记录来源；导入时明确提示并允许跳过 |
