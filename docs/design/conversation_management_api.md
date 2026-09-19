# 设计草案：会话（conversation）管理 API 补齐

> **状态**：草案 / Proposed（2026-09-19，待评审）
> **关联**：[`server_api_completeness.md`](./server_api_completeness.md)（§2.2「运维生命周期」、§2.3 门槛 6「资源生命周期」）、[`web_api_integration.md`](./web_api_integration.md)、[`client_contract_versioning.md`](./client_contract_versioning.md)
> **契约真源**：[`docs/命令行与路由.md`](../命令行与路由.md)（路由与鉴权矩阵）、[`GET /openapi.json`](../openapi.json) 快照、[`docs/配置说明.md`](../配置说明.md)（配置项）
> **非目标**：多租户账号体系与会话级配额（仍按 [`server_api_completeness.md`](./server_api_completeness.md) §2.1 边界由网关/BFF 承担）；用服务端会话索引取代 Client 侧栏；审计日志与会话内容检索；把 async job / SSE resume 状态外置。

---

## 1. 背景（现状事实）

会话资源目前已经具备「创建、读取、分叉」三面，缺「列举、删除、保留策略可配」。

**可用入口**

| 能力 | 入口 | 关键实现 |
|------|------|----------|
| 隐式创建 / 续写 | `POST /chat`、`POST /chat/stream`、`POST /chat/async` 带 `conversation_id`；SSE 回 `x-conversation-id` | [`save_conversation_messages_if_revision`](../src/web/app_state.rs#L333) |
| 读取 | `GET /conversation/messages?conversation_id=…` | 路由 [routes/chat/mod.rs](../src/web/routes/chat/mod.rs#L29)；查询契约 [cm_api_contract/chat.rs](../src/cm_api_contract/chat.rs#L327) |
| 分叉 / 截断 | `POST /chat/branch` | [app_state.rs](../src/web/app_state.rs#L454-L467) |
| 切换存储后端 | `POST /config/session/conversation-store` | [session_conversation_store.rs](../src/web/chat_handlers/session_conversation_store.rs) |

**存储与生命周期**

- 表结构 `crabmate_conversations`：`id` / `messages_json` / `revision` / `updated_at_unix` / `active_agent_role` / `active_session_mode` / `layout_meta_json` —— **无** `title`、**无** workspace 维度、**无** `created_at`、**无**消息条数（[migrate](../src/conversation_store.rs#L33-L49)）。
- 保留策略是**编译期常量**：TTL 24h、最多 512 条（[conversation_store.rs](../src/conversation_store.rs#L21-L23)），仅在**每次保存的尾部**顺带 prune（[L294](../src/conversation_store.rs#L294)、[L421](../src/conversation_store.rs#L421)）；内存后端同规则（[app_state.rs](../src/web/app_state.rs#L25)、[L314-L331](../src/web/app_state.rs#L314-L331)）。SQLite 路径**没有**后台清理任务。
- 删除能力已落地但**未暴露 HTTP**：`delete_by_id`（[conversation_store.rs](../src/conversation_store.rs#L303-L306)）、`delete_conversation_record`（内存 / SQLite 双后端，[app_state.rs](../src/web/app_state.rs#L528-L546)），当前唯一调用方是 e2e 夹具（[routes/e2e_fixtures/mod.rs](../src/web/routes/e2e_fixtures/mod.rs#L96)）。
- **存储作用域是进程级、不是工作区级**：SQLite 连接在启动时按 `conversation_store_sqlite_path` 打开一次（[cli_run.rs](../src/cli_run.rs#L442-L467)，默认相对路径 [default_config.toml](../config/default_config.toml#L101)），**不随 `POST /workspace` 重解析**；因此 `conversation_id` 是进程内全局命名空间，同一张表混存多个工作区的会话。
- 冲突语义已有：`CONVERSATION_CONFLICT`（[conflict.rs](../src/web/chat_handlers/conflict.rs#L9)），SSE 行见 [#L24](../src/web/chat_handlers/conflict.rs#L24)。
- 附图回收会扫描会话正文里仍被引用的 `/uploads/<filename>`（[referenced_upload_filenames](../src/web/app_state.rs#L472-L506)），删除会话会让其附图进入既有清理范围。

**Client 侧已有独立会话索引**（重要前提）：侧栏行由 Client 全量落盘、服务端透传存储，公开投影含 `id` / `title` / `updated_at` / `pinned` / `starred` / `server_conversation_id` / `server_revision` / `workspace_root`（[cm_api_contract/sessions.rs](../src/cm_api_contract/sessions.rs#L3-L39)）。

---

## 2. 缺口判定

| 缺口 | 判定 | 理由 |
|------|------|------|
| **删除会话** | **应补（P0）** | 唯一删除策略是隐式 TTL/容量淘汰：无法立即删除敏感会话，也无法在 24h 不活跃后继续保留；底层能力已就绪，只差一层 HTTP |
| **保留策略可配置** | **应补（P1）** | TTL 与条数上限是常量，运维不可调；且 SQLite 仅在 save 时 prune，空闲进程的过期行不会被清理 |
| **列举会话** | **暂缓（P2，有前置）** | Client 侧栏已持有 title/pin/star/workspace 索引；服务端补列表要先做 schema 迁移并解决跨工作区可见性（§3.3） |
| **重命名 / 设置标题** | **不采纳** | 标题权威在 Client（侧栏 `title`）；服务端无该列，加列即双写，必然不一致 |
| **会话级置顶 / 收藏** | **不采纳** | 同属 Client 侧栏语义（`pinned` / `starred`），服务端不重复存储 |

---

## 3. 设计

### 3.1 D1（P0）`DELETE /conversation/{conversation_id}`

受保护 API（与 `/conversation/*` 同类，鉴权行为沿用[路由矩阵](../命令行与路由.md)）。

| 情况 | 响应 |
|------|------|
| 会话存在 | **204**，删除内存 / SQLite 对应行 |
| 会话不存在或已过期 | **204**（幂等；与 `delete_by_id`「不存在时静默成功」一致） |
| `conversation_id` 非法（空 / 超 `CONVERSATION_ID_MAX_LEN`） | **400** `ApiError` |

**并发（回合进行中被删除）**：删除优先，不引入 409 或会话锁。进行中的回合结束时走 `UPDATE … WHERE id = ? AND revision = ?`，影响 0 行 → 既有 `SaveConversationOutcome::Conflict` 路径 → 客户端收到 `CONVERSATION_CONFLICT`。这样避免把会话删除与 `chat_queue` / SSE hub 耦合；代价是「被删会话正在生成的那一轮结果落不回去」，符合删除语义。

**副作用**：被删会话的附图不再被 `referenced_upload_filenames` 收集，交既有上传清理回收；**不**级联删 Client 侧栏行（属另一存储，由 Client 决定）。

### 3.2 D2（P1）保留策略可配置

新增配置（含义与命名待评审，§7）：

| 键 | 默认 | 含义 |
|----|------|------|
| `conversation_store_ttl_secs` | `86400` | **0 = 不过期** |
| `conversation_store_max_entries` | `512` | **0 = 不限条数** |

- 同步补 `CM_CONVERSATION_STORE_TTL_SECS` / `CM_CONVERSATION_STORE_MAX_ENTRIES`，写入 [default_config.toml](../config/default_config.toml) 与 [`docs/配置说明.md`](../配置说明.md)。
- 两值在每次 save 时读取即可热更；内存后端目前用编译期常量（[app_state.rs](../src/web/app_state.rs#L25)、[L314-L331](../src/web/app_state.rs#L314-L331)）需改为读配置快照，并在 [hot_reload.rs](../src/cm_config/hot_reload.rs) 注明「可热更，上限调小于下一次 save 生效」。
- 补一次低频 prune（启动时 + 定时），解决 SQLite 空闲进程不清理的问题。
- `0` 表示「不限」，与 `--llm-context-tokens 0`（不覆盖）语义不同，文档必须显式写明避免歧义。

### 3.3 D3（P2，暂缓）`GET /conversations`

前置条件是**作用域**：连接为进程级单例、表无 workspace 列，直接列举会跨工作区泄露。若要做，按 expand/contract 分步：

1. expand：新增可空 `workspace_root` 列 + 索引；
2. 双写：save 时写入当前 `chat_workspace_root`；
3. 历史行留 NULL，视为「未知作用域」，按 `workspace_root` 过滤时排除；
4. 契约：`GET /conversations?workspace_root=…&limit&before_updated_at`，返回 `id` / `revision` / `updated_at_unix` / `active_agent_role` / 消息条数；**不**由服务端派生 `title`。

**触发条件**（满足其一再做）：出现不依赖官方 Client 的第二消费者；或出现「跨设备 / 跨浏览器会话列表」需求。当前 Client 侧栏索引已覆盖该场景。

### 3.4 不采纳项

见 §2 表格末两行；另**不**在本设计内引入会话级 TTL 豁免（与侧栏 `pinned` 语义不同，后者仅影响排序），本轮以「显式删除 + 可调保留窗口」覆盖。

---

## 4. 兼容性与约束

- 新增路由不与既有 path/method 冲突。须同步：[openapi_paths.rs](../src/web/openapi/openapi_paths.rs) 补条目、`docs/openapi.json` 快照再生成（`CRABMATE_BLESS=1 cargo test --lib web::openapi::tests::openapi_docs_snapshot_matches_spec`）；路由与 OpenAPI 对齐已有自动测试。
- Client 契约：删除属**可选**新增能力，服务端不假设所有客户端调用；若官方 Client 需要，按 [`client_contract_versioning.md`](./client_contract_versioning.md) 走 Client 侧钉版流程。
- 与 [`server_api_completeness.md`](./server_api_completeness.md) 的关系：本设计只落地其 §2.3 门槛 6 中「会话数据的 TTL / 清理」一项，不改变 §2.1 产品边界；实现后回填 §2.2「运维生命周期」的结论。
- 安全：删除响应不得回显会话正文；日志沿用现状（`target: "crabmate"` 只记 `conversation_id`，无正文）。

---

## 5. 测试计划

- handler：存在 → 204；不存在 → 204（幂等）；非法 id → 400；Bearer 开启时未带密钥 → 401。
- 并发：回合进行中删除 → 回合落盘得 `CONVERSATION_CONFLICT`，且行保持删除态（不复活）。
- 后端矩阵：内存与 SQLite 各跑一遍（沿用 [test_serve.rs](../src/test_serve.rs) 与 e2e 夹具 facet 基建）。
- 保留策略：`ttl=0` 不淘汰；TTL 调小后下一次 save 触发 prune；`max_entries` 淘汰最旧（扩展 [conversation_store.rs](../src/conversation_store.rs) 现有 prune 单测）。
- 契约：OpenAPI 快照 + 路由对齐测试。

---

## 6. 分期与验收

| 期 | 内容 | 验收 |
|----|------|------|
| **P0** | `DELETE /conversation/{id}` + 幂等语义 + OpenAPI/文档 | 上表 handler 测试全绿；内存与 SQLite 行为一致 |
| **P1** | TTL / 上限配置化 + SQLite 低频 prune | 配置生效矩阵测试；`docs/配置说明.md` 与 `default_config.toml` 同步 |
| **P2** | 列举（触发式，含 schema 迁移） | 需另开迁移方案评审 |

---

## 7. 待评审问题

1. 删除不存在的 `conversation_id`：**204 幂等**（草案倾向）还是 404 `CONVERSATION_NOT_FOUND`（与 GET 对齐）？
2. 保留策略配置放在现有 `[agent]` 段还是新开 `[conversation]` 段？
3. 删除时是否要**立即**清理仅被该会话引用的 uploads，还是维持现状（等既有清理周期）？
4. 默认 TTL 24h 意味着「不活跃一天的会话自动消失」，与「会话管理」直觉冲突：是否把默认改为**不过期**，由运维显式设置？
