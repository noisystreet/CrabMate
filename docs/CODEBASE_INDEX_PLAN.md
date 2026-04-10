**语言 / Languages:** 中文（本页）· [English](en/CODEBASE_INDEX_PLAN.md)

# 工作区统一代码索引与增量缓存 — 产品化支持计划

**修订（实现进展）**：`codebase_semantic_search` 所用 SQLite 已增加 **`crabmate_codebase_files`** 文件目录表（`size` / `mtime_ns` / `content_sha256`）；**整库** `rebuild_index` 默认**增量**跳过未改文件，**子目录** `path` 仍为子树替换；`.rs` 嵌入文本附带轻量符号提示。schema **v4** 起增加 **FTS5**（**`crabmate_codebase_chunks_fts`** 外挂块表），**`query`** 默认 **hybrid**（BM25 + 向量加权，见 **`codebase_semantic_hybrid_alpha`** 等）。详见 `docs/DEVELOPMENT.md` 与 `docs/TOOLS.md`。

本文描述「把整个仓库源码 + 元数据做成**持久、可增量更新**的统一索引，以加速代码浏览与检索」的方向性规划。实现时须与 **工作区路径安全**（`resolve_for_read` / 白名单根、`..` 与符号链接策略）、**密钥与日志脱敏**、以及 **P0 Web 鉴权**（多租户隔离）对齐；详见 `.cursor/rules/security-sensitive-surface.mdc` 与 `docs/TODOLIST.md` 全局 P0。

---

## 1. 目标与非目标

### 1.1 目标

- **持久索引**：在进程重启后仍可复用，避免每轮重复全树扫描或反复嵌入同一文件。
- **增量更新**：以文件粒度（及可选目录快照）检测变更，只重算受影响分片；支持大仓。
- **统一入口**：元数据（清单、语言统计、依赖图摘要、可选 git 头指针）与**可检索内容**（全文 / 向量块）在同一套存储与版本语义下管理。
- **加速浏览**：为模型与工具提供低开销的「列目录 / 搜符号 / 搜片段 / 语义近邻」能力，减少盲目 `read_file` 与重复 IO。

### 1.2 非目标（首期可不承诺）

- 替代语言服务器（LSP）的跨文件类型推断与跳转定义精度。
- 实时亚秒级与 IDE 相当的 FS 监听（可作为后续增强）。
- 在未解决 **P0 鉴权** 前，为 Web 提供跨用户共享的全局索引路径（默认应按 **工作区 + 租户键** 隔离）。

---

## 2. 与现有能力的关系

| 现有能力 | 关系 |
|---------|------|
| `ReadFileTurnCache` | 单轮易失缓存；索引是**跨轮持久**层，二者可并存，写工具或 `workspace_changed` 应使相关索引条目失效或入队重扫。 |
| 长期记忆（SQLite + 可选 fastembed） | 面向**对话事实**，scope 多为会话；代码索引建议 **独立 store**（独立表或独立文件），避免与 chat 记忆混用同一 `scope_id` 语义。 |
| `project_profile` | 已覆盖部分**元数据**；索引构建可**复用**其扫描边界与 ignore 规则，或在其输出之上扩展「文件指纹表 + 块表」。 |
| `repo_overview` / `glob_files` / `read_file` | 索引建成后，新增 **`codebase_search`**（或扩展现有工具）应优先查索引，miss 时再回退磁盘。 |

---

## 3. 概念架构

建议分为四层（可在同一 SQLite 或多个文件中实现，但**对外一个「索引运行时」句柄**）：

1. **清单层（Manifest）**  
   - 工作区根规范化路径、索引格式版本、构建时配置指纹（包含 `max_file_bytes`、语言包含列表、ignore 规则哈希）。  
   - 可选：`git HEAD`、索引完成时间、后台任务游标。

2. **文件级元数据（File catalog）**  
   - `rel_path`（工作区内相对路径）、`size`、`mtime_ns`、内容哈希（可选 BLAKE3/SHA256）、语言/类型猜测。  
   - 用于增量：哈希或 `(mtime, size)` 未变则跳过解析与嵌入。

3. **内容层（Chunks）**  
   - 分块策略：按行窗口、AST 感知（远期）、或固定 token 近似长度；存 `raw_text` 或压缩 BLOB、起止行号。  
   - **FTS5**（关键词）与 **向量**（语义）可共用 chunk id，便于混合检索（与 `TODOLIST` 中「长期记忆：检索质量」方向一致，可共享技术选型）。

4. **检索与编排**  
   - 内部 API：`hybrid_query(workspace_id, query, top_k, filters)`。  
   - 暴露为 **工具**（Function Calling）+ 可选 **首轮/按需系统注入摘要**（需严格 `max_chars`，避免撑爆上下文）。

---

## 4. 持久化与默认位置

- **推荐**：工作区内 **`.crabmate/codebase_index/`**（或单文件 `codebase_index.sqlite`），与现有 `.crabmate` 约定一致；路径必须经过与工作区工具相同的 **canonical + 根边界** 校验。  
- **禁止**：将索引文件解析到工作区外（除非显式配置且同样走 `workspace_allowed_roots`）。  
- **忽略**：默认尊重 `.gitignore` + 仓库内 `.crabmateignore`（若尚无，可新增与文档同步）；明确排除 `.env`、密钥路径模式、超大二进制与 `node_modules`/`target` 等（可配置）。

---

## 5. 增量更新策略

1. **全量冷启动**：首次启用或版本升级时后台或显式命令全量遍历（可限并发、限 CPU）。  
2. **增量**：扫描 catalog，比较 `mtime/size` 或内容哈希；变更文件重新分块并更新 FTS/向量。  
3. **失效事件**：与 `read_file` 缓存一致，在**写文件、删除、`workspace_changed`** 时标记脏范围或入队重扫。  
4. **可选**：`notify`/`watch`  debounce（二期）；**定时**或 **chat 空闲时** 合并重扫（降低峰值）。  
5. **Git**：可选以 `HEAD` 变更触发「变更路径集合」缩小扫描面（不能替代 mtime，因 checkout 外编辑存在）。

---

## 6. 安全与合规（必须写入验收标准）

- 所有索引路径解析复用 **`canonical_workspace_root` + `resolve_for_read` 语义**，不新增绕过入口。  
- 索引内容**不落日志**全文；错误信息不含用户源码片段。  
- **`.env`、常见密钥文件名、`.pem`** 等默认排除；可配置扩展排除 glob。  
- Web 多用户场景：**索引与查询必须绑定**「会话/工作区/租户」键，与 P0 鉴权方案同时交付或默认仅本地 CLI 启用。

---

## 7. 分阶段里程碑（建议）

### Phase 0 — 设计与脚手架

- 确定存储形态（SQLite 单库 vs 目录下「catalog + sqlite」）。  
- 配置草案：`codebase_index_enabled`、`codebase_index_path`（可选）、`codebase_index_max_file_bytes`、`codebase_index_exclude_globs`、后台构建开关。  
- 文档：`README.md` / `docs/DEVELOPMENT.md` / `docs/TOOLS.md` 占位说明（实现各 phase 时同步补全）。

### Phase 1 — 文件目录与元数据（无向量）

- 持久化 File catalog + 可选 tokei/依赖摘要缓存。  
- CLI / Web：**手动或启动时触发** `index rebuild`；`/status` 暴露 `codebase_index_*` 就绪与滞后信息。  
- 新工具：**`codebase_grep_indexed`** 或等价（底层 FTS 或内存倒排），验证增量更新正确性。

### Phase 2 — 分块 + FTS 混合检索

- Chunk 表 + FTS5；查询 API 与工具参数（路径前缀、扩展名过滤、`top_k`）。  
- 与 `run_agent_turn` 集成：可选「首轮注入索引摘要」或仅工具按需调用（优先后者以降低噪音）。

### Phase 3 — 向量嵌入与混合打分

- 复用 **fastembed**（与长期记忆同后端时统一 ONNX 加载与线程模型）；chunk 级嵌入 + 余弦检索。  
- **混合排序**：FTS 分数 + 向量分数加权（可调）；记录降级路径（embed 失败时仅 FTS）。  
- 大仓：**批处理队列**、可取消、可配置并发。

### Phase 4 — 体验与运维

- Web：索引状态、进度、失败重试、显式「重建」按钮（受鉴权保护）。  
- 与 **外部向量库**（Qdrant/pgvector）可选对接（与 `TODOLIST` 长期记忆条目同向，可共享客户端抽象）。  
- Benchmark：索引构建耗时、查询 P95、对 `read_file` 调用次数的减少（内部基准即可）。

---

## 8. 测试与验收

- **单元测试**：路径遍历边界、ignore、增量检测逻辑、符号链接（若允许）行为与文档一致。  
- **集成测试**：临时工作区 fixture：增删改文件后索引条目与检索结果一致。  
- **安全回归**：尝试将索引路径指向工作区外应失败；敏感文件不出现在检索结果中。

---

## 9. 开放问题（需在 Phase 0 拍板）

- 索引默认 **自动后台** 还是 **仅手动**（大仓首次 CPU  spike 与用户预期）。  
- 是否与 **MCP / 外部 IDE** 共享索引格式（大概率否，保持内部 SQLite 即可）。  
- **CLI vs Web** 是否共用同一 `.crabmate` 索引文件（默认是，但 Web 多工作区切换时需清晰 `workspace_id`）。

---

## 10. 文档与清单维护

- 实现某一阶段后：在 **`docs/TODOLIST.md`** 中删除已完成的细项或合并重复项，并在本文件顶部「修订记录」中简述（或依赖 Git 历史）。  
- 新增工具与配置键时遵守 `.cursor/rules/todolist-and-documentation.mdc`。

---

*本文档为规划性质；具体 API 与配置名以实现时为准。*
