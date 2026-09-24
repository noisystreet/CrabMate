# 设计草案：工具管理 API 补齐

> **状态**：草案 / Proposed（2026-09-20）。**尚未评审、尚未实现**。§7.1 是本轮给出的建议默认取值（P0 契约口径，需确认后才算冻结）；§7.2 为遗留待评审。
> **关联**：[`memory_management_api.md`](./memory_management_api.md)（同一批「资源补口」的姊妹设计，禁用态/幂等口径对齐它）、[`tool_calling_evolution.md`](./tool_calling_evolution.md)（工具面演进总清单）、[`background_tool_jobs_contract.md`](./background_tool_jobs_contract.md)、[`server_api_completeness.md`](./server_api_completeness.md)、[`user_data_dir.md`](./user_data_dir.md)
> **契约真源**：[`docs/命令行与路由.md`](../命令行与路由.md)（路由与鉴权矩阵）、[`docs/openapi.json`](../openapi.json) 快照、[`docs/命令行契约.md`](../命令行契约.md)（错误码表）、[`docs/配置说明.md`](../配置说明.md)（配置项）、[`src/cm_api_contract/error_codes.rs`](../../src/cm_api_contract/error_codes.rs)
> **非目标**：MCP 服务器管理（`/user-data/mcp-servers*` 12 端点已完备）；工具**执行**通道（`POST /tools/{name}/invoke` 之类，见 §3.6）；审批策略与 `tool_approval` 语义变更；`ToolSpec` 静态表结构改造；子代理工具放行策略变更；前端工具面板（官方 UI 在 [`crabmate-client`](https://github.com/noisystreet/crabmate-client)，本仓只负责契约）。

---

## 1. 背景（现状事实）

工具面**没有统一的 HTTP 观测与管理入口**：模型侧能用的工具集合由「编译期静态表 + 每回合运行期拼接 + 若干零散特判」决定，人类侧却只能靠间接迹象推断。本节逐项列举既有事实（以代码为准）。

### 1.1 工具面的四类来源

| 来源 | 注册时机 | 命名 | 关键实现 |
|------|----------|------|----------|
| **内置工具** | 编译期静态表 | 原名（如 `run_command`） | [tool_specs_registry/mod.rs](../../src/cm_tools/tools/tool_specs_registry/mod.rs#L13-L46) |
| **工作区动态工具** | 每回合扫描 `<workspace>/plugins/*.json` | `dyn__` 前缀 | [dynamic_tools.rs](../../src/cm_internal/dynamic_tools.rs#L17-L41) |
| **MCP 工具** | 每回合按配置开/复用会话后合并 | `mcp__{slug}__{remote}` | [agent_turn_prep.rs](../../src/cm_internal/agent_turn_prep.rs#L64-L76) |
| **skills** | 不进工具表；经元工具 `skill_manage`（install/list/remove）与 composer `/` 斜杠 | — | [skill_manage.rs](../../src/cm_tools/tools/skill_manage.rs) |

四类工具的**共同事实**：没有任何一处「启用/禁用」状态位——「在场」只由构建期特性门与回合拼接逻辑决定。

### 1.2 工具清单是编译期固化的

- [`ToolSpec`](../../src/cm_tools/tools/runners.rs#L32-L39) 字段为 `name` / `description` / `category` / `parameters` / `runner` / `summary`，**没有 `enabled`，也没有任何策略字段**；`parameters` 是 `fn() -> Value` 构建器，`runner` 是函数指针。
- 注册表由 27 个 `include!("specs/*.inc.rs")` **静态数组**经 `OnceLock` 拼接为 `&'static [ToolSpec]`（[tool_specs_registry/mod.rs](../../src/cm_tools/tools/tool_specs_registry/mod.rs#L13-L46)），另有 `find_spec` 的 O(1) 名字索引（[cm_tools/tools/mod.rs](../../src/cm_tools/tools/mod.rs)）。
- 结论：`&'static` 表**无法承载运行期启停状态**，任何启停只能落在「构建/回合拼接」阶段。

### 1.3 运行期过滤链（两段，且都缺配置来源）

**构建期**（[cm_tools/tools/mod.rs](../../src/cm_tools/tools/mod.rs)）：`ToolsBuildOptions { categories, dev_tags }` → `tool_passes_filters`，外加 `codebase_semantic_search` 的 `fastembed` 特性门（[runners.rs](../../src/cm_tools/tools/runners.rs#L42-L44)）。但全仓检索显示 `ToolsBuildOptions` 的**唯一生产侧引用是 `lib_server.rs` 的 re-export**，其余是测试；`src/cm_config` 内检索 `dev_tags` / `tool_categories` **无任何匹配**——即 `categories` / `dev_tags` 两个过滤维度**根本没有配置入口**，当前恒为「不过滤」。

**回合期**（[prepare_tools_for_turn](../../src/cm_internal/agent_turn_prep.rs#L57-L101)，其唯一调用点在 [run_agent_turn.rs](../../src/run_agent_turn.rs)）：

1. 合并工作区动态工具（L64-67）；
2. 尝试开/复用 MCP 会话并合并远端工具（L68-76）；
3. 若 `codebase_semantic_search_enabled=false`，按名剔除 `codebase_semantic_search`（L77-79）；
4. 若 `long_term_memory_enabled=false`，按名剔除 `long_term_remember` / `long_term_forget` / `long_term_memory_list`（L80-87）；
5. 若本回合带工具白名单，按其收窄（L88-95）。

第 3/4 步是**两个彼此独立的硬编码特判**：新增一个「某工具可由配置关闭」的需求，就要再插一段 `retain`。这既是可维护性问题，也是「工具面不可观测」的根因——除了读源码，没人能说出当前有哪些工具在场、为什么某项不在场。

### 1.4 `[tool_registry]`：已有完整的「行为策略」面，零「启停」语义

[`ToolRegistryPolicyConfig`](../../src/cm_config/types/agent_config_sections.rs#L268-L304) 共 22 个字段，**全部是「怎么写 / 怎么并发 / 怎么重试」**，没有一项控制「哪些工具在场」：

| 分组 | 字段（节选） | 语义 |
|------|--------------|------|
| 写类工具集 | `write_effect_tools` | 判定写盘效应（[registry_policy.rs](../../src/cm_tools/registry_policy.rs#L94) 内置 44 项，含 `skill_manage`） |
| 并行拒绝集 | `parallel_sync_denied_tools` / `parallel_sync_denied_prefixes` | 禁止并行（[L175](../../src/cm_tools/registry_policy.rs#L175) 内置 8 项精确 + 17 前缀） |
| 内联集 | `sync_default_inline_tools` | 免 spawn（[L281](../../src/cm_tools/registry_policy.rs#L281) 内置 2 项） |
| 墙上时钟 | `parallel_wall_timeout_secs` 等 3 项 | 外圈超时覆盖 |
| 后台任务 | `background_jobs_enabled` 等 8 项 | 见 [`background_tool_jobs_contract.md`](./background_tool_jobs_contract.md) |
| 透明重试 | `tool_retry_enabled` 等 6 项 | 瞬时失败重试 |
| 子代理放行 | `sub_agent_*_extra_tools` / `_deny_tools` | 分阶段子代理的额外放行/拒绝 |

有利条件：该段**已在热重载白名单内**（[hot_reload.rs](../../src/cm_config/hot_reload.rs) 整体 `clone_from`），且在 [validate.rs](../../src/cm_config/validate.rs) 有范围校验先例（如 `1..=86400`），用户在 [config/tools.toml](../../config/tools.toml#L195-L242) 已见惯 `[tool_registry]` 段。**因此新增启停字段的改造成本极低；缺的只是字段本身。**

### 1.5 已完备的部分（本设计不重复造）

- **后台工具任务**：`GET /tools/jobs/{id}`、`GET /tools/jobs/{id}/output?cursor=`、`POST /tools/jobs/{id}/cancel`（[routes/tools/mod.rs](../../src/web/routes/tools/mod.rs#L12-L26)），含 `X-Workspace-Root` 归属校验、`JOB_NOT_FOUND`(404) / `JOB_EXPIRED`(410) / `JOB_OWNERSHIP_MISMATCH`(403) 与 cancel 的 409 语义（[命令行契约.md](../命令行契约.md#L67-L77)）。
- **MCP 服务器管理**：12 条 `/user-data/mcp-servers*` 路由（[routes/user_data/mod.rs](../../src/web/routes/user_data/mod.rs#L20-L64)），覆盖读写、导入、状态、批量探测、远端鉴权与单点探测。
- **skills 只读**：`GET /skills`（[skills_handlers.rs](../../src/web/skills_handlers.rs#L10-L68)），响应 `enabled` / `skills_dir` / `skills_user_dir` / `skills_system_dir` / `skills: [{id,name,description,path}]` / `error`（[cm_web_host/http_types/skills.rs](../../src/cm_web_host/http_types/skills.rs#L15-L25)）；`enabled=false` 时回 **200 + `enabled:false`** 而非 503；错误经 `sanitize_skills_list_error` 脱敏（引号内以 `/` 或 `\` 开头的串替换为 `(path)`，[L70-L95](../../src/web/skills_handlers.rs#L70-L95)）。
- **skills 写入**：模型侧元工具 `skill_manage` 已能 install/list/remove（三层 system → user → workspace）。
- **动态工具**：仅 CLI 三命令 `crabmate plugin init | list | validate`（[runtime/cli/commands.rs](../../src/runtime/cli/commands.rs)），**无任何 HTTP 通道**。

### 1.6 观测面与文档/契约漂移

- **观测面缺口**：模型可读的 `self_config_info` 中，`tool_registry_section` **只输出 2 个字段**（`background_jobs_enabled`、`parallel_wall_timeout_overrides_count`，[self_config_info.rs](../../src/cm_tools/tools/self_config_info.rs#L266-L273)），22 字段里的策略集合与超时表全部不可见。
- **文档漂移（双缺口）**：[`docs/命令行与路由.md`](../命令行与路由.md) 自称路由真源，但其受保护 API 鉴权矩阵与路由表**均无 `/tools/jobs/*`**；该端点契约目前只存在于 `docs/openapi.json` 快照、[openapi_paths_tool_jobs.rs](../../src/web/openapi/openapi_paths_tool_jobs.rs) 与 `docs/命令行契约.md`。
- **错误码缺口**：[error_codes.rs](../../src/cm_api_contract/error_codes.rs) 共 23 条常量，**无任何 `TOOL_*` / `PLUGIN_*` / `SKILLS_*`**；`SKILL_INVOKE_FAILED` 只以字面量出现在 handler 与 `命令行契约.md` 表中，未登记为常量。
- **OpenAPI tag 缺口**：顶层 `tags` 数组为 chat / workspace / system / tasks / tool_jobs / config / user_data / uploads（[openapi/mod.rs](../../src/web/openapi/mod.rs#L34-L43)），**没有 `skills`**，但 `/skills` 片段自带 `"tags": ["skills"]`（[openapi_paths_workspace.rs](../../src/web/openapi/openapi_paths_workspace.rs#L151-L171)）。
- **待办缺口**：[`待办清单.md`](../待办清单.md) 的 `tools/` 章（L99-L107）只列了长耗时执行、审批分级、新栈扩展与 MCP 扩展，**没有「工具清单 / 启停 / 统一收口」条目**。

---

## 2. 缺口判定

| 缺口 | 判定 | 理由 |
|------|------|------|
| **工具清单 HTTP 入口**（`GET /tools`） | **应补（P0）** | 「当前有哪些工具在场、来自哪个源、为什么不在场」目前只能读源码回答；四类来源已全部可就地枚举，只差一层只读投影 |
| **单工具详情 + 策略投影**（`GET /tools/{name}`） | **应补（P1）** | `[tool_registry]` 22 字段与 `registry_policy` 的判定结果（只读/写类/可并行/内联/可后台/可重试）已有公开函数可调，却完全不可见；这是 §1.6 观测面缺口的直接补口 |
| **单工具启停**（`PUT /tools/{name}/enabled`） | **应补（P1）** | 现在关一个工具必须改代码（加 `retain` 特判）或整体关掉某子系统的全部工具（如 `long_term_memory_enabled=false` 连带关三个工具）；「临时停用某个危险工具」是最常见的运维诉求 |
| **启停的配置落点**（`[tool_registry] disabled_tools`） | **应补（P1）** | 是 §1.4 已就绪段落的自然延伸：热重载白名单、范围校验先例、TOML 段位置都已具备 |
| **工作区动态工具 HTTP 只读列表** | **应补（P1）** | `plugins/*.json` 现在是黑盒：加载失败一律 `log::warn!` 后静默跳过（[dynamic_tools.rs](../../src/cm_internal/dynamic_tools.rs#L83-L90)），用户看不到「文件写错在哪」 |
| **工作区动态工具 HTTP 写入** | **暂缓（P2）** | 写盘 + 命令白名单双重安全面（`command` 必须命中 `allowed_commands`），需单独定路径守卫与冲突语义 |
| **skills 单条启停** | **不采纳（本轮）** | 语义不清：同名 skill 在 system / user / workspace 三层可并存，禁用哪一层需先定消歧规则；而 `skill_manage` 的 remove 已能覆盖「不要它」的主诉求 |
| **工具执行 / 直调 HTTP 入口** | **不采纳** | `POST /tools/{name}/invoke` 会绕过审批、回合预算与 changelist，安全面上不接受（见 §3.6） |
| **进程级「工具总开关」** | **不采纳** | 没有这个概念，也没有真实诉求；引入只会多一个与 `[agent]` 语义重叠的开关（见 §3 约定与 §3.6） |
| **MCP 服务器管理** | **已完备** | §1.5，12 端点齐备 |
| **前端工具面板** | **不在本仓** | 官方 UI 在 `crabmate-client`；本设计只保证契约稳定 |

---

## 3. 设计

统一约定：

- 全部为**受保护 API**（Web Bearer / `X-API-Key`），归类与 `/tools/jobs/*` 一致，同步更新[鉴权矩阵](../命令行与路由.md)。
- **命名空间**：`/tools` 与 `/tools/{tool_name}` 与既有 `/tools/jobs/{tool_job_id}` **段数不同**（2 段 vs 3 段），axum 不会冲突。注意 `GET /tools/jobs` 会落到「名为 `jobs` 的工具」→ **404 `TOOL_NOT_FOUND`**（现状该路径同样 404），此边界需在文档显式记录。
- **工具名定位参数统一为路径段 `{tool_name}`**：校验长度 ≤ 128 且字符集为 `[A-Za-z0-9_.:-]`（可覆盖 `dyn__` 与 `mcp__{slug}__{remote}`），非法 → **400 `INVALID_TOOL_NAME`**；未注册 → **404 `TOOL_NOT_FOUND`**。
- **投影不回显敏感/可注入字段**：列表接口不回显动态工具的 `command` / `args`（`args` 由用户填写，可能含路径或凭据）；`parameters`（JSON Schema）本就是给模型的公开内容，**仅在单工具详情**回显，避免清单响应体积失控。
- **不引入进程级工具总开关**，因此本设计**没有 503 禁用态**。这与 [`memory_management_api.md`](./memory_management_api.md) 的 `503 LONG_TERM_MEMORY_DISABLED` 口径不同，是刻意的：记忆有独立的 `long_term_memory_enabled` 总开关，工具面没有（也不该有）。skills 相关端点沿用既有 `200 + enabled:false`（§1.5）。
- 工具名一律按**精确匹配**处理（区分大小写），与 `registry_policy` 的 `HashSet<String>` 语义一致；前缀通配仅在配置字段 `disabled_tool_prefixes` 中提供。
- 日志只记 `tool_name` / `source` / 变更结果，不记 `description` 全文。

### 3.1 D1（P0）`GET /tools`

查询参数（校验风格对齐 [src/web/http_types/validation.rs](../../src/web/http_types/validation.rs)）：

| 参数 | 必填 | 默认 | 说明 |
|------|------|------|------|
| `source` | 否 | 全部 | 可重复：`builtin` / `dynamic` / `mcp` |
| `category` | 否 | 全部 | `basic` / `development`；非枚举值 → 400 |
| `dev_tag` | 否 | — | 命中 `dev_tag::tags_for_tool_name`（`development` 类才有） |
| `q` | 否 | — | `name` / `description` 子串 |
| `limit` | 否 | `50` | clamp 到 `1..=200` |
| `offset` | 否 | `0` | 与 `limit` 组成切片 |

响应 `ToolsListResponse`：`tools: [ToolListItem]` / `total` / `limit` / `offset` / `has_more` / `counts: {builtin, dynamic, mcp, present}` / `workspace_root: string?`。

`ToolListItem`：`name` / `description` / `category`（`basic` \| `development`；动态与 MCP 工具按 `development` 归入或在 §7.2 另定）/ `source`（`builtin` \| `dynamic` \| `mcp`）/ `dev_tags: [string]` / `present: bool` / `absent_reason: string?`。

`absent_reason` 取值（**只反映既有事实**）：

| 值 | 触发条件 | 现状来源 |
|----|----------|----------|
| `feature_not_built` | 编译期未启用 `fastembed` | [runners.rs](../../src/cm_tools/tools/runners.rs#L42-L44) |
| `config_disabled` | `codebase_semantic_search_enabled=false` / `long_term_memory_enabled=false` | [agent_turn_prep.rs](../../src/cm_internal/agent_turn_prep.rs#L77-L87) |
| `disabled_by_policy` | 命中 `[tool_registry] disabled_tools` / `disabled_tool_prefixes` | **D3 落地后才会出现** |

**实现要点（两条硬约束）**：

1. **只读、无副作用**。回合期的 `try_open_session_and_tools` 会**打开或复用 MCP 会话**——`GET /tools` 绝不能调用它。MCP 部分只允许读**已缓存的会话快照**与**已配置的 server 列表**（与 `GET /user-data/mcp-servers/status` 同源）；未建立会话时该源返回空并置 `counts.mcp = 0`。
2. **工作区未设置不是错误**。内置工具不依赖工作区，因此缺工作区时仍回 200：`dynamic` 源为空、`workspace_root: null`。**不**返回 `WORKSPACE_NOT_SET`（与 `/workspace*` 的既有口径不同，此处刻意区分）。

### 3.2 D2（P1）`GET /tools/{tool_name}`

返回 `ToolDetailView`：D1 的字段，外加

- `parameters`：完整 JSON Schema（`cached_params_for_tool_name`，[cm_tools/tools/mod.rs](../../src/cm_tools/tools/mod.rs)）；
- `policy`：**策略归属投影**，是本节主要价值，全部来自 [registry_policy.rs](../../src/cm_tools/registry_policy.rs) 已公开的函数，不新增判定逻辑：

  | 字段 | 来源 |
  |------|------|
  | `read_only` | `is_readonly_tool`（[L161](../../src/cm_tools/registry_policy.rs#L161)） |
  | `write_effect` | `write_effect_tools` 命中（内置 44 项，[L94](../../src/cm_tools/registry_policy.rs#L94)） |
  | `parallel_readonly_batch_allowed` | `tool_ok_for_parallel_readonly_batch_piece`（[L255](../../src/cm_tools/registry_policy.rs#L255)）与并行拒绝集（内置 8 精确 + 17 前缀，[L175](../../src/cm_tools/registry_policy.rs#L175)） |
  | `sync_default_inline` | `sync_default_runs_inline`（[L292](../../src/cm_tools/registry_policy.rs#L292)） |
  | `wall_timeout_secs` | 生效后的墙上时钟（含 `parallel_wall_timeout_secs` 覆盖） |
  | `sub_agent_extra_allow: [string]` | 命中的 `sub_agent_*_extra_tools` 集合名 |
  | `background_job_capable` | 仅 `run_command` 为 `true`（且受 `background_jobs_enabled` 约束）；`background_job_async_tools` 白名单为空时全部为 `false` |
  | `retry_eligible` | 命中 `tool_retry_*` 的准入条件（只读 + 免审批 + 不在 `denied_tools`） |

未注册工具名 → **404 `TOOL_NOT_FOUND`**。

该接口直接补上 §1.6 的观测面缺口，无需再扩写 `self_config_info`。

### 3.3 D3（P1）`PUT /tools/{tool_name}/enabled`

请求体 `{ "enabled": bool }`，**幂等 PUT**（重复写同一值仍 200）。

| 情况 | 响应 |
|------|------|
| 已注册、写入成功 | **200** + `{ name, enabled, source, effective_after_reload: true }` |
| 重复写入同值 | **200**（幂等） |
| `{tool_name}` 字符集/长度非法 | **400** `INVALID_TOOL_NAME` |
| 未注册的工具名 | **404** `TOOL_NOT_FOUND` |

**持久化落点：本机用户数据**，而非 TOML 写回。理由与既有先例一致：

- 新增 `cm_internal/user_data/store.rs` 同目录文件 `tool_overrides.json`（`CM_CRABMATE_USER_DATA_DIR`），与 `prefs.json` / `llm_overrides.json` / `mcp_servers.json` 同级（[store.rs](../../src/cm_internal/user_data/store.rs)）；
- 复用 `apply_user_data_llm_overrides` 的既有管线形态：在 [config_reload.rs](../../src/runtime/config_reload.rs) 与 [cli_run.rs](../../src/cli_run.rs) 两处调用点追加一个 `apply_user_data_tool_overrides(&mut cfg)`，即**同时覆盖启动与 `POST /config/reload` 热路径**；
- 不引入 TOML 程序化写回（避免破坏用户手写注释与格式），TOML 侧仍可由用户静态声明同一个字段。

**配置层新增字段**（`[tool_registry]`）：

```rust
pub tool_registry_disabled_tools: Option<Arc<HashSet<String>>>,        // None = 全部启用
pub tool_registry_disabled_tool_prefixes: Option<Arc<[String]>>,       // 前缀通配
```

合并顺序（后者覆盖前者）：TOML/环境变量 → `tool_overrides.json` 本机覆写。

**生效落点：`prepare_tools_for_turn`**。它是三个 HTTP 入口（`/chat`、`/chat/async`、`/chat/stream`）的**唯一**汇合点（[run_agent_turn.rs](../../src/run_agent_turn.rs)），在现有 5 步之后追加第 6 步：

```
if let Some(disabled) = cfg.tool_registry_policy.tool_registry_disabled_tools { … retain … }
else if !prefixes.is_empty() { … retain … }
```

**不合并既有特判**：`codebase_semantic_search` 与 `long_term_memory_*` 两处 `retain` 保持原样（行为等价性优先，避免把「配置开关」与「本机策略」混成一条链）；D1/D2 的 `absent_reason` 对两者分别识别即可。

**语义边界**：禁用只影响**本会话后续回合的工具在场性**，不追溯已产生的历史工具调用；被禁用工具的既有 `tool_job` 轮询/取消端点仍可正常访问（`/tools/jobs/*` 与在场性无关）。

### 3.4 D4（P2，暂缓）工作区动态工具（`plugins/*.json`）HTTP 读写

- `GET /tools/plugins`（**只读列表，建议提前到 P1**）：`file` / `name` / `description` / `valid: bool` / `error: string?` / `command_allowed: bool`。直接暴露 §2 表格中「加载失败静默跳过」的问题。
- `GET /tools/plugins/{file}`：含 `parameters` / `args` / `pass_args_json`。
- `PUT /tools/plugins/{file}`（创建/覆盖）与 `DELETE /tools/plugins/{file}`（**204 幂等**）。
- 服务端复用同一套校验：`validate_file` 的 4 条（`dyn__` 前缀 / `description` 非空 / `parameters` 须 JSON 对象 / `command` 非空，[dynamic_tools.rs](../../src/cm_internal/dynamic_tools.rs#L55-L81)）+ `command` 必须命中 `allowed_commands`；失败 **400 `INVALID_PLUGIN_DEFINITION`**。
- **路径守卫**：`{file}` 仅允许 `[A-Za-z0-9_-]+\.json`，显式拒绝分隔符与 `..`（对齐 `/workspace/file` 的既有风格）。
- 暂缓理由：写盘 + 命令白名单双重安全面，且需先定「同名文件覆盖是否需要 `confirm`」与 changelist 归属口径。

### 3.5 D5（不采纳，本轮）skills 单条启停

`GET /skills` 已只读完备（§1.5）；`skill_manage` 已能 install/list/remove。补 `PUT /skills/{id}/enabled` 需要先回答「三层同名 skill 禁用哪一层」，且禁用与删除的差别（留给模型的斜杠是否仍出现）不明。触发条件：出现明确的「保留但不启用」工单后再立项。

### 3.6 不采纳项

- **`POST /tools/{name}/invoke`（HTTP 直调工具）**：绕过 `tool_approval`、回合预算与 changelist，安全面不接受。
- **进程级「工具总开关」与 503 禁用态**：无此概念，不引入（见 §3 约定）。
- **给 `ToolSpec` 加 `enabled` 字段**：`&'static` 表承载不了运行期状态，且会污染编译期注册表（§1.2）。
- **把 `categories` / `dev_tags` 接上配置**：与本设计的「启停」正交，且当前**无真实需求驱动**（§1.3）；真要做应单独立项，不与启停混在同一字段上。
- **MCP 服务器管理 / 后台任务端点**：已完备（§1.5）。
- **前端工具面板**：不在本仓。

---

## 4. 兼容性与约束

- **路由对齐是硬约束**：[route_table.rs](../../src/web/openapi/route_table.rs) 递归扫描 `src/web/routes/**`（跳过 `e2e_fixtures`）**外加** `src/web/server.rs` 与 `cm_web_host/routes/web_ui.rs`，与 OpenAPI `paths` 双向比对（`openapi_ops_match_axum_route_source`）。新增 `/tools*` 条目**必须**同步 OpenAPI，否则该测试直接失败。建议新增 `src/web/openapi/openapi_paths_tools.rs`（现仅有 `openapi_paths_tool_jobs.rs`，两者可共用前缀）并挂到 `openapi_paths_value()`（[openapi_paths.rs](../../src/web/openapi/openapi_paths.rs) 的 `merge_path_fragments` **遇重复 path key 会 panic**，勿重复注册 `/tools/jobs/*`），随后重生成快照：
  `CRABMATE_BLESS=1 cargo test --lib web::openapi::tests::openapi_docs_snapshot_matches_spec`
- **同步清单**：
  - [routes/mod.rs](../../src/web/routes/mod.rs) 更新 `tools` 子模块的表行说明（现为「`/tools/jobs/*` 后台工具任务轮询与取消」，需扩展到清单/启停）；
  - [server.rs](../../src/web/server.rs#L16-L24) `protected_api` **已 merge `routes::tools`**，若沿用同一子模块则**无需改动**；
  - [`docs/命令行与路由.md`](../命令行与路由.md) 补鉴权矩阵类别与路由表行，**并顺带补齐缺失的 `/tools/jobs/*` 三行**（§1.6 既有漂移，同一 PR 内修正）；
  - 错误码写入 [error_codes.rs](../../src/cm_api_contract/error_codes.rs) 并对齐 [`docs/命令行契约.md`](../命令行契约.md)：需要 `INVALID_TOOL_NAME`(400)、`TOOL_NOT_FOUND`(404)、`INVALID_PLUGIN_DEFINITION`(400，D4)；**顺带**把已在契约表中但不存在的 `SKILL_INVOKE_FAILED` 补为常量（§1.6）；
  - OpenAPI 顶层 `tags` 补 `skills`（§1.6）。
- **配置与热重载**：`[tool_registry]` 已在 [hot_reload.rs](../../src/cm_config/hot_reload.rs) 的 `clone_from` 白名单内，新增字段**无需改热重载代码**；在 [validate.rs](../../src/cm_config/validate.rs) 追加「工具名格式」校验（对齐既有 `1..=86400` 一类范围校验的写法）；[`docs/配置说明.md`](../配置说明.md) 与 [config/tools.toml](../../config/tools.toml#L195-L242) 同步补注释样例。
- **不改变既有行为**：`prepare_tools_for_turn` 前 5 步的顺序与语义不变；`ToolSpec` 与 `tool_specs_registry` 不动；`/tools/jobs/*` 三端点行为与字段不变；`GET /skills` 响应不变；`self_config_info` 输出不变（观测改由 D2 提供，不扩写该工具）。
- **安全**：列表接口不回显 `command` / `args`；`{file}` 走白名单字符集守卫；启停写的是**本机** `~/.local/share/crabmate`（与工作区策略无关，多用户共享工作区时是「各自生效」——见 §7.2）；日志不记 `description` 全文。

---

## 5. 测试计划

- **handler（D1）**：三源合并计数与 `counts` 自洽；`source` / `category` / `dev_tag` / `q` 过滤各自命中与叠加；分页切片（`limit` / `offset` / `has_more` / `total`）；超 `limit` 被 clamp；**`absent_reason` 三值各自命中**（`feature_not_built` 需在未启用 `fastembed` 的构建下断言）；**无工作区 → 200 且 `dynamic` 为空**（反向断言：不得返回 `WORKSPACE_NOT_SET`）；**`GET /tools` 不建立 MCP 会话**（断言无副作用，可用 mock/计数桩）；Bearer 开启未带密钥 → 401。
- **handler（D2）**：`run_command` 的 `policy` 全字段与 `registry_policy` 判定一致（写类/可并行/内联/后台/重试各一条断言）；`sync_default_inline` 命中 `get_current_time`；未知名 → 404；非法名 → 400。
- **handler（D3）**：写入后立即读 D1 该工具 `present:false` / `absent_reason:"disabled_by_policy"`；重复写入同值 → 200 且文件内容不变（幂等）；前缀通配命中多个工具；`POST /config/reload` 后仍生效；`tool_overrides.json` 重启后仍生效；非法名 → 400、未注册名 → 404；被禁用工具的 `/tools/jobs/*` 访问不受影响（反向断言）。
- **handler（D4，P2）**：只读列表能报出「`parameters` 非对象」这类校验失败（对应现状静默跳过）；`{file}` 含 `..` / `/` / 非 `.json` → 400；`command` 不在白名单 → 400；`DELETE` 幂等 204。
- **配置层**：`apply_user_data_tool_overrides` 的合并优先级单测（TOML 被本机覆写覆盖）；`validate.rs` 对新字段的格式校验。
- **契约**：`openapi_ops_match_axum_route_source` + `openapi_docs_snapshot_matches_spec` 两个既有测试转绿。
- **不新增跨仓 e2e**：Playwright 用例归 `crabmate-client`。

---

## 6. 分期与验收

| 期 | 内容 | 验收 |
|----|------|------|
| **P0** | `GET /tools` + 三源枚举（**只读、不建会话**）+ 路由/OpenAPI/文档同步（含补齐 `/tools/jobs/*` 路由表行） | §5 中 D1 用例与契约测试全绿；`cargo clippy --all-targets --all-features -- -D warnings` 通过 |
| **P1** | `GET /tools/{tool_name}`（策略投影）+ `PUT /tools/{tool_name}/enabled`（`tool_overrides.json` + `[tool_registry] disabled_tools` + `prepare_tools_for_turn` 第 6 步）+ `GET /tools/plugins` | D2/D3 用例全绿；热重载与重启两条路径均有断言 |
| **P2** | `plugins` 写入/删除、`parameters` 体量与分页策略再评估 | 需另开安全评审（路径守卫 + `confirm` 语义 + changelist 归属） |

---

## 7. 评审结论与遗留问题

### 7.1 本轮建议默认取值（待确认；P0 契约口径）

| # | 问题 | 建议取值 | 影响 |
|---|------|----------|------|
| Q1 | 路径形态 | `/tools`（清单）+ `/tools/{tool_name}`（详情/启停），`{tool_name}` 走路径段；`GET /tools/jobs` 落 404 `TOOL_NOT_FOUND` | §3 全部接口形态；无需引入嵌套路由解析 |
| Q2 | 是否需要禁用态（503） | **不需要**：本设计不引入进程级工具总开关；skills 沿用既有 `200 + enabled:false` | §3 约定；**不新增** `TOOLS_DISABLED` 一类错误码 |
| Q3 | 启停持久化落点 | **本机用户数据** `tool_overrides.json`（复用 `apply_user_data_*` 管线）**优先于** TOML 字段 | §3.3；不改 TOML 写回路径 |
| Q4 | 启停生效落点 | **`prepare_tools_for_turn` 第 6 步**（唯一汇合点，覆盖三个 HTTP 入口）；不合并既有两个特判 | §3.3；`run_agent_turn.rs` 不改 |
| Q5 | 是否给 `ToolSpec` 加 `enabled` | **不加**；启停只走配置名单 + 回合过滤 | 注册表保持编译期 `&'static` |
| Q6 | 清单是否回显 `parameters` | **不回显**（仅 D2 单工具详情回显） | 控制清单体积；与「不回显 `command` / `args`」并列 |

### 7.2 遗留待评审

1. **启停的作用域**：`tool_overrides.json` 在 `~/.local/share/crabmate`，是**本机/本进程**语义。多人共享同一 `serve` 实例时，「我关掉 `run_command`」会影响他人；是否需要在工作区级（`<workspace>/.crabmate/`）也提供一份覆写？若需要，需先定优先级与冲突提示。
2. **`dynamic` / `mcp` 工具的 `category` 归类**：动态与 MCP 工具没有 `ToolCategory`，D1 现在需要替它们选一个（建议 `development`）。若后续要让它们在 `basic` 场景也可见，需扩 `ToolCategory` 或改为可空。
3. **`codebase_semantic_search_enabled` / `long_term_memory_enabled` 是否迁入新名单**：迁入能让「关工具」只剩一条语义，但会改变既有配置的等价性与文档口径（§3.3 明确本轮不动）。
4. **禁用名的前缀通配是否够用**：`disabled_tool_prefixes` 可一次关掉整栈（如 `cargo_*`），但也会误伤；是否需要 `re` 或显式列举约束需按真实工单定。
