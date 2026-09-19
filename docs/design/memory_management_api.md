# 设计草案：长期记忆（memory）管理 API 补齐

> **状态**：草案 / Proposed（2026-09-19）。**P0 契约已评审确认**（路径形态、禁用态、删除幂等、与会话删除的关系、对外枚举命名，见 §7.1）；**尚未开始实现**。§7.2 三条遗留问题只影响 P1，或已在 §3 / §4 给出默认取值。
> **关联**：[`conversation_management_api.md`](./conversation_management_api.md)（同一批「资源生命周期」补口的姊妹设计，已实现）、[`memory_todo.md`](./memory_todo.md)（记忆可视化面板与未来扩展清单）、[`server_api_completeness.md`](./server_api_completeness.md)、[`summarize_experience.md`](./summarize_experience.md)
> **契约真源**：[`docs/命令行与路由.md`](../命令行与路由.md)（路由与鉴权矩阵）、[`docs/openapi.json`](../openapi.json) 快照、[`docs/配置说明.md`](../配置说明.md)（配置项）、[`src/cm_api_contract/error_codes.rs`](../../src/cm_api_contract/error_codes.rs)（错误码）
> **非目标**：多租户身份与跨 scope 租户键（沿用 [`待办清单.md`](../待办清单.md) L80 的前置条件，仍由网关/BFF 承担）；Qdrant / pgvector 向量后端接线；检索质量（FTS5 混合检索、query 构造）；语义分块与近重复合并；前端 `MemoryModal`（官方 UI 在 [`crabmate-client`](https://github.com/noisystreet/crabmate-client) 仓，本仓只负责契约）；`long_term_remember` / `long_term_forget` 等模型侧工具的行为变更。

---

## 1. 背景（现状事实）

长期记忆（LTM）目前**只有模型侧工具通道，没有任何人类可用的 HTTP 入口**——这与会话资源在本次 HEAD 改动前的状态同构（会话当时也只有底层删除能力、无 HTTP）。

### 1.1 可用入口（全部为 LLM 内置工具）

| 能力 | 工具 | 关键实现 |
|------|------|----------|
| 写入（显式） | `long_term_remember` | [long_term_memory_tools.rs](../../src/cm_internal/long_term_memory_tools.rs#L45) → [explicit_remember_blocking](../../src/cm_memory/memory/long_term_memory.rs#L675) |
| 写入（经验沉淀） | `summarize_experience` | [long_term_memory_tools.rs](../../src/cm_internal/long_term_memory_tools.rs#L104) → [summarize_experience_remember_blocking](../../src/cm_memory/memory/long_term_memory.rs#L687) |
| 删除 | `long_term_forget`（按 `memory_id` 或 `memory_text`） | [explicit_forget_blocking](../../src/cm_memory/memory/long_term_memory.rs#L735) |
| 列举 | `long_term_memory_list`（返回**纯文本**，非 JSON） | [list_recent_blocking](../../src/cm_memory/memory/long_term_memory.rs#L766) |

工具在 `long_term_memory_enabled=true` 时注册（[agent_turn_prep.rs](../../src/cm_internal/agent_turn_prep.rs#L80-L87)），宿主按 `scope_id` 构造（[memory_tool_hosts.rs](../../src/cm_internal/memory_tool_hosts.rs#L76-L91)）。

### 1.2 作用域（scope）

- **`scope_id` 就是 Web 的 `conversation_id`**：`long_term_memory_scope_id: Some(conversation_id.to_string())`（[lib_server.rs](../../src/lib_server.rs#L297)）。
- 非 Web 路径传 `None` 时不挂记忆宿主（[tool_context_memory_extras](../../src/cm_memory/memory/long_term_memory.rs#L883-L899)）。
- LTM 表**与外层无外键**，`scope_id` 是普通列；因此记忆行可以**比会话行活得更久**。

### 1.3 存储（[long_term_memory_store.rs](../../src/cm_memory/memory/long_term_memory_store.rs)）

- 表 `crabmate_long_term_memory`：`id` / `scope_id` / `chunk_text` / `source_role` / `created_at_unix` / `embedding`，增量迁移补 `expires_at_unix` / `tags_json`（默认 `'[]'`）/ `source_kind`（默认 `'auto'`）；索引 `idx_crabmate_long_term_memory_scope_created(scope_id, created_at_unix DESC)`（[migrate](../../src/cm_memory/memory/long_term_memory_store.rs#L42-L72)）。
- 现状是 **CRD（Create / Read / Delete），缺 Update**：

| 操作 | 函数 |
|------|------|
| Create | [insert_chunk](../../src/cm_memory/memory/long_term_memory_store.rs#L151)、[insert_explicit_chunk](../../src/cm_memory/memory/long_term_memory_store.rs#L179) |
| Read | [list_for_scope](../../src/cm_memory/memory/long_term_memory_store.rs#L133)、[list_recent_for_scope](../../src/cm_memory/memory/long_term_memory_store.rs#L299) |
| Delete | [delete_by_id_for_scope](../../src/cm_memory/memory/long_term_memory_store.rs#L248)、[delete_matching_text](../../src/cm_memory/memory/long_term_memory_store.rs#L261)、[delete_oldest_beyond](../../src/cm_memory/memory/long_term_memory_store.rs#L208)、[delete_expired_for_scope](../../src/cm_memory/memory/long_term_memory_store.rs#L93) |
| **Update** | **不存在** |

- 列表读取能力不足以直接支撑 HTTP 投影：
  - `list_recent_for_scope` 只回 `MemoryListRow = (id, chunk_text, source_kind, expires_at_unix, tags_json)`——**丢掉了 `created_at_unix` 与 `source_role`**（[L286-L314](../../src/cm_memory/memory/long_term_memory_store.rs#L286-L314)）；
  - `list_for_scope` 字段齐全，但 SELECT **包含 `embedding` BLOB**（[L141-L148](../../src/cm_memory/memory/long_term_memory_store.rs#L141-L148)），当 HTTP 投影会白读大对象；
  - 两者都**无分页 / 过滤 / 排序 / 总数**参数。
- 过期行在读写前被动清理（各函数头部调用 `delete_expired_for_scope`），**无后台任务**；`max_entries` 淘汰只在写入路径触发（[delete_oldest_beyond](../../src/cm_memory/memory/long_term_memory_store.rs#L208)）。

### 1.4 运行时与对外可见性

- `LongTermMemoryRuntime`：`conn: Arc<Mutex<Connection>>` + 可选 `Mutex<Option<TextEmbedding>>` + `pub index_errors: AtomicU64`（[long_term_memory.rs](../../src/cm_memory/memory/long_term_memory.rs#L140-L170)），已注入 `AppStateWebAux.long_term_memory: Option<Arc<LongTermMemoryRuntime>>`（[app_state.rs](../../src/web/app_state.rs#L155)）。
- **唯一对外可见的记忆信息在 `/status`**：`long_term_memory_enabled` / `long_term_memory_vector_backend` / `long_term_memory_store_ready` / `long_term_memory_index_errors`（[health_status.rs](../../src/web/chat_handlers/health_status.rs#L139-L145)、[L398-L410](../../src/web/chat_handlers/health_status.rs#L398-L410)）。
- 路由层**完全空白**：[routes/mod.rs](../../src/web/routes/mod.rs#L16-L25) 的 11 个子模块无 `memory`；[server.rs](../../src/web/server.rs#L16-L24) 的 `protected_api` merge 链无记忆路由。
- `DELETE /conversation/{conversation_id}`（HEAD `1c9f64f6`）**只删会话记录**（[routes/chat/mod.rs](../../src/web/routes/chat/mod.rs#L30)、[chat_handlers/chat/mod.rs](../../src/web/chat_handlers/chat/mod.rs#L204-L230)），**不**触碰 LTM 表——删会话后该 scope 的记忆仍在。

### 1.5 已有设计与待办

- [`memory_todo.md`](./memory_todo.md) 是最早的设计草案（§1-§8 为基础面板 + `GET /memory/list` / `DELETE /memory/:id` / `GET /memory/stats`），**至今未实现**；其 §14 另列导入/导出/修改等未来扩展。
- 该草案 §4 称「现有 `list_recent_for_scope` 已返回所需字段，无需改表结构」——**与实际不符**：该函数不返回 `created_at_unix` / `source_role`（见 §1.3）。本文档以代码为准。
- [`待办清单.md`](../待办清单.md#L84) 明确列出「长期记忆：运营与合规 API（只读列表 / 按 scope 删除）」为**未完成**。

---

## 2. 缺口判定

| 缺口 | 判定 | 理由 |
|------|------|------|
| **只读列表 HTTP 入口** | **应补（P0）** | 排障与被遗忘权类需求都要求「不经过模型」直接看某 scope 存了什么；底层读能力已就绪，只差一层 HTTP 与一个完整投影 |
| **按 id 删除 HTTP 入口** | **应补（P0）** | `explicit_forget_blocking` / `delete_by_id_for_scope` 已就绪，仅缺路由；会话侧已有先例（`DELETE /conversation/{id}`） |
| **存储层 Update** | **应补（P1）** | CRUD 缺口中最实的一项：目前「改错一个字」只能删了重记，会丢 `created_at` 并可能撞 `has_duplicate_text` 去重；但 HTTP 修改涉及 embedding 重算，排在列表/删除之后 |
| **统计（stats）** | **应补（P1）** | `/status` 只有进程级开关与错误计数，scope 级规模（条数 / 类型分布 / 到期时间）无处可查；合规与容量排查都需要 |
| **按 scope 批量清空** | **暂缓（P2）** | 「被遗忘权」语义上必要，但破坏性最强，需先定确认机制与审计口径；且单条删除已能覆盖多数场景 |
| **导出 / 导入** | **暂缓（P2）** | `memory_todo.md` §14 的未来扩展；跨部署迁移尚无真实需求 |
| **语义压缩 / freshness / 置信度** | **不采纳（本轮）** | 属检索质量与模型侧策略，见 [`待办清单.md`](../待办清单.md#L81-L83)，与本设计正交 |
| **前端 `MemoryModal`** | **不在本仓** | 官方 UI 在 `crabmate-client`；本设计只保证契约稳定，面板按 [`client_contract_versioning.md`](./client_contract_versioning.md) 在 Client 侧推进 |

---

## 3. 设计

统一约定：

- 全部为**受保护 API**（Web Bearer / `X-API-Key`），归类与 `/conversation/*` 一致，同步更新[鉴权矩阵](../命令行与路由.md)。
- **定位 scope 的参数名统一为 `conversation_id`**，取值语义即 §1.2 的 `scope_id`；校验复用 [normalize_client_conversation_id](../../src/web/chat_handlers/parse.rs#L307-L326)（长度上限 128、字符集 `[A-Za-z0-9-_.:]`），非法返回 **400 `INVALID_CONVERSATION_ID`**。
- 条目投影 `MemoryEntryView`：`id` / `text` / `kind`（对外**稳定枚举**）/ `tags`（数组，`tags_json` 解析失败按 `[]` 处理，与既有 `unwrap_or('[]')` 一致）/ `created_at_unix` / `expires_at_unix`（`null` = 不过期）。**绝不返回 `embedding`**，也**不返回**内部 `source_role` / `source_kind` 原始字符串（§7.1 Q5）。
- `kind` 映射——内部标识只在服务端存在，对外只有下表四个稳定值；未来新增 `source_role` 一律落到 `other`，不得 500：

  | 内部 `source_role` | 内部 `source_kind` | 对外 `kind` |
  |--------------------|--------------------|-------------|
  | `user` / `assistant` | `auto` | `turn` |
  | `explicit` | `explicit` | `explicit` |
  | `summarize_experience` / `auto_summarize_experience` | `explicit` | `experience` |
  | 其他（未知 / 未来新增） | — | `other` |

  （`user` 与 `assistant` 的区分本轮不对外暴露；若面板确需展示，再补稳定字段 `turn_role`，同样不泄漏内部字符串。）
- `scope` 不存在 == 该 scope 无记忆，返回 **200 + 空列表**（LTM 没有「scope 注册」概念，无法区分「从未写入」与「已清空」）。
- 日志只记 `conversation_id` / `id` / 条数，不记正文。

### 3.1 D1（P0）`GET /memory/list`

查询参数（校验风格对齐 [`src/web/http_types/validation.rs`](../../src/web/http_types/validation.rs)）：

| 参数 | 必填 | 默认 | 说明 |
|------|------|------|------|
| `conversation_id` | 是 | — | scope；非法 → 400 |
| `limit` | 否 | `50` | clamp 到 `1..=200` |
| `offset` | 否 | `0` | 与 `limit` 组成切片 |
| `kind` | 否 | — | 取值同 `MemoryEntryView.kind`（`turn` / `explicit` / `experience` / `other`）；非枚举值 → 400 `INVALID_MEMORY_KIND` |
| `tag` | 否 | — | 命中 `tags_json` 中的标签 |
| `q` | 否 | — | `chunk_text` 子串（不做 FTS；见 §3.6） |
| `sort` | 否 | `created_at_desc` | 或 `created_at_asc` |

响应 `MemoryListResponse`：`conversation_id` / `entries: [MemoryEntryView]` / `total`（过滤后总数）/ `limit` / `offset` / `has_more`。

**实现要点**：不直接复用 `list_recent_for_scope`（字段不足，§1.3）。存储层新增列表函数，SELECT 显式列出投影所需列并**排除 `embedding`**，同时新增同过滤条件的 `count`。过滤/排序在 SQL 内完成，分页用 `LIMIT ? OFFSET ?`。

### 3.2 D2（P0）`DELETE /memory/{id}`

| 情况 | 响应 |
|------|------|
| 行存在且属于该 `conversation_id` | **204**，删除对应行 |
| 行不存在 / 已过期 / **不属于**该 scope | **204**（幂等，与 HEAD 的 `DELETE /conversation/{id}` 一致） |
| `conversation_id` 非法 | **400** `INVALID_CONVERSATION_ID` |
| `{id}` 非十进制整数 | **400** `INVALID_MEMORY_ID` |

**`conversation_id` 为必填查询参数**（作用域守卫）：底层 `delete_by_id_for_scope` 本就按 scope 收窄，路由层要求同名参数可避免「用 A 会话的 id 删掉 B 会话记忆」；归属不符时回 204 而非 403，避免泄露他 scope 的 id 是否存在。

### 3.3 D3（P1）`GET /memory/stats`

查询参数 `conversation_id` **可选**：给出时回该 scope 明细，省略时只回进程级信息（与 `/status` 已有字段同源，避免两处漂移）。

响应 `MemoryStatsResponse`：

- 进程级：`enabled` / `store_ready` / `vector_backend` / `index_errors`（与 [health_status.rs](../../src/web/chat_handlers/health_status.rs#L139-L145) 同源）；
- scope 级（给出 `conversation_id` 时）：`total` / `by_kind`（`turn` / `explicit` / `experience` / `other` 四个计数，与 §3 的 `kind` 枚举同源）/ `expiring_soon`（如 24h 内到期）/ `next_expires_at_unix` / `oldest_created_at_unix` / `newest_created_at_unix`；
- 策略：`max_entries` / `default_ttl_secs`（来自 `[long_term_memory]` 配置，见 [`config/memory.toml`](../../config/memory.toml)）。

**只回计数与时间戳，不回正文**。

### 3.4 D4（P1）`PUT /memory/{id}`（补齐 Update）

请求体：`conversation_id`（必填）、可选 `text` / `tags` / `ttl_secs` / `clear_ttl`。

| 变更 | 行为 |
|------|------|
| 仅 `tags` / TTL | 直接 `UPDATE`，**不**触碰 `embedding` |
| `text` 变更 | 必须**重算 embedding**（`embedding` 与新正文不符会让向量检索命中错位）；复用 [explicit_chunk_embedding_bytes](../../src/cm_memory/memory/long_term_memory.rs#L195-L231) |
| fastembed 未启用 | 向量列置 `NULL`（关键词检索仍可用），计数进 `index_errors`，响应体带 `index_degraded: true` |
| `ttl_secs: 0` | 置 `NULL`（不过期） |
| `clear_ttl: true` | 置 `NULL`；与 `ttl_secs` 同时出现 → **400** |
| `id` 不属于该 scope | **404** `MEMORY_NOT_FOUND` |
| 文本与同 scope 既有行重复 | 沿用 `has_duplicate_text` 语义 → **409** `MEMORY_DUPLICATE_TEXT` |

**不可变字段**：`source_kind`、`source_role`、`created_at_unix`。编辑**不**刷新 `created_at_unix`（不改变排序与新鲜度），也**不**新增 `updated_at_unix` 列——本轮不引入 schema 迁移（见 §7.2）。

存储层新增 `update_chunk_for_scope`，采用「`Option` 参数 = 不改」语义，避免调用方拼接 SQL。

### 3.5 D5（P2，暂缓）批量清空与导入导出

- `DELETE /memory?conversation_id=…`（按 scope 清空）：需先定**确认机制**（如必带 `confirm=true`）、**审计口径**（记谁在何时清了哪个 scope、影响行数）与**与 `DELETE /conversation/{id}` 的关系**（§7.1 Q3）。
- 导出 / 导入：JSONL 形态，导入需定 scope 重写规则；且 `kind` 无法反推内部 `source_role`，导入落到哪个 role 需一并评审。

触发条件：出现明确的「被遗忘权」工单，或跨部署迁移需求。

### 3.6 不采纳项

见 §2 末三行。另**不**在本设计内做：把 `list_recent_blocking` 的纯文本输出改为 JSON（模型侧工具契约稳定优先，HTTP 投影另开函数）；引入 FTS5 / 语义压缩等检索质量改动。

---

## 4. 兼容性与约束

- **路由对齐是硬约束**：[route_table.rs](../../src/web/openapi/route_table.rs#L14-L18) 会递归扫描 `src/web/routes/**` 的 `.route(` 调用，与 OpenAPI `paths` 做双向比对（`openapi_ops_match_axum_route_source`）。新增 `/memory/*` **必须**同步 OpenAPI 条目，否则该测试直接失败。建议新增 `src/web/openapi/openapi_paths_memory.rs` 并挂到 `build_openapi_spec()`，随后再生成快照：
  `CRABMATE_BLESS=1 cargo test --lib web::openapi::tests::openapi_docs_snapshot_matches_spec`
- **同步清单**：[routes/mod.rs](../../src/web/routes/mod.rs#L16-L25) 补 `memory` 子模块与文档表格行；[server.rs](../../src/web/server.rs#L16-L24) `protected_api` 追加 `.merge(routes::memory::router())`；[`docs/命令行与路由.md`](../命令行与路由.md) 补鉴权矩阵类别与路由表行；错误码写入 [error_codes.rs](../../src/cm_api_contract/error_codes.rs) 并对齐 `docs/命令行契约.md`——P0 需要 `INVALID_MEMORY_ID`、`INVALID_MEMORY_KIND` 与 `LONG_TERM_MEMORY_DISABLED`；`MEMORY_NOT_FOUND` 仅 D4（P1）的 `PUT` 未命中使用（`DELETE` 已定为 204 幂等，见 §7.1 Q4）。
- **禁用态语义**：`long_term_memory_enabled=false` **或** `AppState.long_term_memory` 为 `None` 时（[cli_run_serve.rs](../../src/cli_run_serve.rs#L34-L70) 在内存后端且未配 `long_term_memory_store_sqlite_path` 时会跳过 runtime），三个路由统一回 **503 `LONG_TERM_MEMORY_DISABLED`**（§7.1 Q2 已定），响应体带 `enabled: false` 便于 UI 直接展示原因。
- **不改变既有行为**：`DELETE /conversation/{id}` 仍**不**清理 LTM（§1.4）；模型侧四个工具的行为与输出格式不变；`/status` 既有字段不变。
- **schema**：D1 / D2 / D3 **不改表结构、不加索引**（现有 `scope_created` 索引可支撑 `created_at` 排序与 `scope_id` 收窄）；D4 只 `UPDATE` 既有列。`kind` 过滤在 SQL 内表达为 `source_role IN (…)`（`turn` → `user`/`assistant`；`experience` → 两个经验 role；`other` → `NOT IN` 取反）。若实测 `source_role` / `tag` 过滤走全表扫描，再单独评估 `(scope_id, source_role, created_at_unix DESC)` 索引。
- **顺带修正（同一 PR 内）**：[`docs/配置说明.md`](../配置说明.md#L259) 称注入前缀统一为 `[记忆 #id]`，实际按 `source_role` 分三种（`【经验 #{id}】` / `【显式记忆 #{id}】` / `[记忆 #{id}]`，[long_term_memory_recall.rs](../../src/cm_memory/memory/long_term_memory_recall.rs#L129-L136)）；注入头文案也漏了 `【显式记忆 #id】`（[long_term_memory.rs](../../src/cm_memory/memory/long_term_memory.rs#L425)）。本设计新增的 `kind`（`experience` / `explicit` / `turn`）与三种注入前缀一一对应，会让该不一致更显眼，建议一并校正文档。
- **安全**：响应不回显 embedding；日志不记正文（与 §1.4 现状一致）；删除/修改不返回被操作行内容。

---

## 5. 测试计划

- **handler（D1）**：分页切片（`limit`/`offset`/`has_more`/`total`）、`kind` 过滤（四个枚举值各自命中；非法值 → 400；未知 `source_role` 落 `other` 而非 500）、`tag` 过滤、`q` 子串、`sort` 两个方向、空 scope → 200 空列表、`conversation_id` 非法 → 400、超 `limit` 被 clamp、Bearer 开启未带密钥 → 401。
- **handler（D2）**：命中 → 204 且行消失；重复删除 → 204；**跨 scope 的合法 id → 204 但目标行仍在**（作用域守卫的反向断言）；`{id}` 非数字 → 400。
- **handler（D3）**：无 `conversation_id` 只回进程级字段；给出后计数与 D1 的 `total` 一致；`expiring_soon` 边界（刚好等于阈值）。
- **handler（D4）**：仅改 tags → `embedding` 字节不变；改 text → 重新写入 embedding（fastembed 开/关两条路径）；`ttl_secs=0` 与 `clear_ttl` 均置 `NULL`；二者并存 → 400；不可变字段未被改写；重复文本 → 409。
- **禁用态**：`enabled=false` 与 `runtime=None` 两条路径均回 503（或评审结论）。
- **存储层单测**：扩展 [long_term_memory_store.rs](../../src/cm_memory/memory/long_term_memory_store.rs#L316-L341) 现有 `#[cfg(test)]` 模块，覆盖新列表函数（投影列正确、过期行被过滤、`offset` 与 `count` 一致）与 `update_chunk_for_scope` 的各 `Option` 分支。
- **契约**：`openapi_ops_match_axum_route_source` + `openapi_docs_snapshot_matches_spec` 两个既有测试转绿。
- **不新增跨仓 e2e**：Playwright 用例归 `crabmate-client`。

---

## 6. 分期与验收

| 期 | 内容 | 验收 |
|----|------|------|
| **P0** | `GET /memory/list` + `DELETE /memory/{id}` + 存储层「完整投影列表 + 计数」（排除 `embedding`）+ 路由/OpenAPI/文档同步 | §5 中 D1/D2 用例与契约测试全绿；`cargo clippy --all-targets --all-features -- -D warnings` 通过 |
| **P1** | `GET /memory/stats` + `PUT /memory/{id}`（含 embedding 重算与降级计数） | D3/D4 用例全绿；fastembed 开/关两条路径行为有断言 |
| **P2** | 按 scope 批量清空、导出/导入 | 需另开方案评审（确认机制 + 审计 + 与 `DELETE /conversation/{id}` 的关系） |

---

## 7. 评审结论与遗留问题

### 7.1 已定结论（2026-09-19）

| # | 问题 | 结论 | 影响 |
|---|------|------|------|
| Q1 | 路径形态 | **扁平** `/memory/list` + `/memory/{id}`（scope 走查询参数 `conversation_id`），沿用 [`memory_todo.md`](./memory_todo.md) 原草案 | §3 全部接口形态不变；不引入嵌套路由解析 |
| Q2 | 禁用态 | **503 `LONG_TERM_MEMORY_DISABLED`**，响应体带 `enabled: false` | §4 禁用态语义生效；新增该错误码 |
| Q3 | 与会话删除的关系 | **不级联**，本轮维持现状（记忆可比会话活得久，无外键） | [conversation_delete_handler](../../src/web/chat_handlers/chat/mod.rs#L204-L230) **不改**；[`conversation_management_api.md`](./conversation_management_api.md) 无需回填 |
| Q4 | 删除幂等 | **204 幂等**（不存在 / 已过期 / 不属于该 scope 均 204，不泄露 id 是否存在） | §3.2 表即为最终语义；**不新增** `MEMORY_NOT_FOUND` 错误码 |
| Q5 | `source_kind` / `source_role` 对外命名 | **收敛为稳定枚举** `kind`（`turn` / `explicit` / `experience` / `other`），内部标识不外泄 | §3 投影与映射表生效；过滤参数由 `source_kind` 改为 `kind`；`source_role` 不出现在响应中 |

### 7.2 遗留待评审

1. **编辑语义**：`created_at_unix` 保持不变（草案倾向，不引 schema 迁移）是否可接受？还是需要 `updated_at_unix` 列以支持「最近编辑」排序与审计？（只影响 P1 的 D4）
2. **`conversation_id` 是否强制作域守卫**：本文档 D2/D4 要求必填（防跨 scope 误删）。若未来引入多租户身份，该参数可由服务端从身份推导，届时再简化。
3. **`turn` 是否细分**：`user` / `assistant` 的区分本轮不暴露（§3）。若面板需要「你说过 / 我说过」的展示，再补稳定字段 `turn_role`。
