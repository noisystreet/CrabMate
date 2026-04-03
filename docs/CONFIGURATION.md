**语言 / Languages:** 中文（本页）· [English](en/CONFIGURATION.md)

# 配置说明

默认配置由仓库 **`config/default_config.toml`**、**`config/session.toml`**、**`config/context_inject.toml`**、**`config/tools.toml`**、**`config/sandbox.toml`**、**`config/planning.toml`**、**`config/memory.toml`** 七段嵌入（各段主体为 **`[agent]`** 扁平键；**`config/tools.toml`** 还可选 **`[tool_registry]`** 表，见下文「`tool_registry` 策略」；**`session`** 为 CLI 会话相关 **`tui_*`** 与 **`repl_initial_workspace_messages_enabled`**；**`context_inject`** 为首轮 **`agent_memory_file_*`**、**`project_profile_inject_*`**、**`project_dependency_brief_inject_*`**；**`tools`** 的 **`[agent]`** 含 **`run_command`** 白名单/超时/工作目录、**`tool_message_*`** / **`tool_result_envelope_v1`**、**`read_file_turn_cache_*`**、**`test_result_cache_*`**、**`session_workspace_changelist_*`**、**`codebase_semantic_*`**（**`codebase_semantic_search`** 与写后失效 **`codebase_semantic_invalidate_on_workspace_change`**）、天气/搜索/**`http_fetch_*`**、**`tool_call_explain_*`**、**`mcp_*`** 等；**`sandbox`** 为 **SyncDefault Docker 沙盒** **`sync_default_tool_sandbox_*`**；**`planning`** 为规划 / 反思 / 编排；**`memory`** 为 **`long_term_memory_*`**）。`load_config` 按 **主默认 → session → context_inject → tools → sandbox → planning → memory** 顺序合并，再被 **`config.toml`** 或 **`.agent_demo.toml`** 覆盖，最后由环境变量覆盖。示例片段见 **`config.toml.example`**。

**未知键与越界数值**：用户 **`config.toml`** / **`agent_roles.toml`** 中 **`[agent]`**、**`[tool_registry]`**、**`[[agent_roles]]`** 等已声明表内若出现**未在 CrabMate 中定义的键**，TOML 解析会失败（serde **拒绝未知字段**），避免拼写错误被静默忽略。对 **`finalize` 中有上下限的数值项**（如 **`temperature`**、**`max_message_history`**、**`chat_queue_max_concurrent`** 等），若在 TOML 或 **`AGENT_*`** 中写出**超出允许范围**的值，启动（及热重载路径上的 **`load_config`**）会返回明确错误，而**不再**仅做静默截断（`clamp`）；详见源码 **`src/config/validate.rs`** 与 **`finalize`** 中的默认值说明。

## 配置热重载（无需重启 `repl` / `serve` 主进程）

- **CLI**：输入 **`/config reload`**（或 Tab 补全 **`/config reload`**）。从与启动时相同的配置文件路径（**`--config`** 或默认探测 **`config.toml`** / **`.agent_demo.toml`**）再读 TOML，并与**当前进程环境变量**合并后，将可热更字段写入内存中的 [`AgentConfig`](DEVELOPMENT.md)；随后清空 MCP 进程内 stdio 缓存，下一轮对话使用新 MCP 指纹。
- **Web**：**`POST /config/reload`**（JSON body 可为 `{}`；鉴权与 **`/chat`** 等受保护 API 一致——若启动时启用了 Bearer 中间件则须带 token）。成功时返回 **`{ "ok": true, "message": "…" }`**。
- **会更新的典型项**：**`api_base`**、**`model`**、**`llm_http_auth_mode`**、**`llm_reasoning_split`**、**`llm_bigmodel_thinking`**、**`llm_kimi_thinking_disabled`**、**`llm_fold_system_into_user`**、**`temperature` / `llm_seed`**、各类**超时与重试**、**`run_command` 白名单**、**`http_fetch_allowed_prefixes`**、**`workspace_allowed_roots`**、**`web_api_bearer_token`**（仅影响 handler 内校验；见下）、**`mcp_*`**、**`[tool_registry]`**（HTTP 外圈超时、并行墙钟覆盖、并行拒绝/内联/写副作用名单）、**`system_prompt_file` 重读**、上下文与规划相关键等（实现见源码 **`apply_hot_reload_config_subset`**）。
- **刻意不热更**：**`conversation_store_sqlite_path`**（会话库连接在启动时打开，改路径须重启 **`serve`**）。**`reqwest::Client`** 不重建，**`api_timeout_secs` 等**对**新连接**的生效可能受连接池保留的空闲连接影响。
- **`API_KEY`**：仍只从**环境变量**读取；热重载**不**解析密钥文件。改 **`API_KEY`** 后通常需**重新 export** 并再执行 **`/config reload`**（或重启进程）以便与 **`llm_http_auth_mode=bearer`** 行为一致。
- **Bearer 中间件层**：若启动 **`serve`** 时 **`web_api_bearer_token` 非空**，Axum 会在该进程生命周期内挂上鉴权层；热重载**不会**拆除或新增该层——**从「无 token」变为「有 token」**或反向时，须**重启 `serve`**。热重载仍会更改 handler 内读取的 token 字符串，用于已挂层时的校验。
- **敏感字段内存表示**：**`web_api_bearer_token`** 与 **`web_search_api_key`** 在 [`AgentConfig`](DEVELOPMENT.md) 内为 **secrecy `SecretString`**，**`Debug` / 结构化日志默认不打印明文**；源码中通过 **`ExposeSecret::expose_secret()`** 取用（`config` crate 再导出 **`ExposeSecret`**）。**`API_KEY`** 仍为仅环境变量，未并入 `AgentConfig`。

## 环境变量（`AGENT_*`）

以下为常用项；**完整键名与默认值以 `config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml` 为准**。**`API_KEY`** 仅环境变量，见下节「模型与 API」表格；热重载与密钥行为见上文「配置热重载」。

### 模型与 API

| 环境变量 | 说明 |
| --- | --- |
| `API_KEY` | 云厂商 / OpenAI 兼容后端的 Bearer token；`llm_http_auth_mode=bearer`（默认）时发往 `chat/completions` / `models` 的 `Authorization`。**不写 TOML**；热重载不解析密钥文件，改值后通常须重新 export 并 **`/config reload`** 或重启进程。`llm_http_auth_mode=none`（如本地 Ollama）时可不设。 |
| `AGENT_API_BASE` | 覆盖 `api_base`。 |
| `AGENT_MODEL` | 覆盖 `model`。 |
| `AGENT_LLM_HTTP_AUTH_MODE` | `bearer`（默认，需 **`API_KEY`**）或 `none`（不向 `chat/completions` / `models` 发 `Authorization`，本地 Ollama 等可不设 **`API_KEY`**）。 |
| `AGENT_LLM_REASONING_SPLIT` | 覆盖 `llm_reasoning_split`。未在 TOML/环境变量设置时：**MiniMax 网关**（`model` 或 `api_base` 可识别为 MiniMax）**默认为开**（`true`），其它网关默认为关；见下文「MiniMax」。 |
| `AGENT_LLM_BIGMODEL_THINKING` | 为真时在请求体中带智谱 **`thinking: { "type": "enabled" }`**（GLM-5 深度思考；见下文「智谱 GLM」）。 |
| `AGENT_LLM_KIMI_THINKING_DISABLED` | 为真时在请求体中带 **`thinking: { "type": "disabled" }`**（关闭 Moonshot **kimi-k2.5** 默认思考；见下文「Moonshot（Kimi）」）。 |
| `AGENT_LLM_FOLD_SYSTEM_INTO_USER` | 为真时将 `system` 并入 `user`（不接受独立 `system` 的网关/代理）。 |
| `AGENT_SYSTEM_PROMPT` | 内联系统提示；会清除继承的 `system_prompt_file`（若再设 `AGENT_SYSTEM_PROMPT_FILE` 则以文件为准，见「系统提示词」）。 |
| `AGENT_SYSTEM_PROMPT_FILE` | 系统提示词文件路径。 |
| `AGENT_DEFAULT_AGENT_ROLE` | 未传 Web `agent_role` / CLI `--agent-role` 时使用的**默认角色 id**（须已在角色表中定义；见下文「多角色」）。 |

### 采样与随机性

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_TEMPERATURE` | 覆盖 `temperature`。 |
| `AGENT_LLM_SEED` | 覆盖 `llm_seed`。 |

### Web 服务

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_HTTP_HOST` | 未传 `--host` 时作为绑定地址。 |
| `AGENT_WEB_API_BEARER_TOKEN` | 受保护 API 的 Bearer token。 |
| `AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK` | 非回环监听时是否允许无鉴权启动（高风险，仅可信环境）。 |

### 工作区与 Cursor 式规则

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_WORKSPACE_ALLOWED_ROOTS` | 逗号分隔，等价 `[agent] workspace_allowed_roots`。 |
| `AGENT_CURSOR_RULES_ENABLED` | 是否启用规则注入。 |
| `AGENT_CURSOR_RULES_DIR` | 规则目录（`*.mdc`）。 |
| `AGENT_CURSOR_RULES_INCLUDE_AGENTS_MD` | 是否并入 `AGENTS.md`。 |
| `AGENT_CURSOR_RULES_MAX_CHARS` | 注入长度上限。 |

**路径安全（与实现一致）**：`workspace_allowed_roots` 与每次请求对当前工作区根的重验可拒绝明显的 `..` 逃逸与**校验时刻**已指向根外的 symlink。**不能消除**「校验通过 → 随后 `open`」之间的 **TOCTOU**：恶意或并发替换路径仍可能导致打开与校验不一致的对象。更强保证需在打开时使用 **`O_NOFOLLOW`**、基于目录 fd 的 **`openat`** 等（见 **`src/path_workspace.rs`** 模块注释、**`docs/TODOLIST.md`** P0）。在不可信工作区或开放网络上须与 **Web 鉴权**（见该文档热重载与 P0 说明）一并评估。

### 规划与分阶段规划

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_FINAL_PLAN_REQUIREMENT` | `never` / `workflow_reflection` / `always`。 |
| `AGENT_PLAN_REWRITE_MAX_ATTEMPTS` | 规划重写上限。 |
| `AGENT_PLANNER_EXECUTOR_MODE` | `single_agent` / `logical_dual_agent`。 |
| `AGENT_STAGED_PLAN_EXECUTION` | 是否启用分阶段规划。 |
| `AGENT_STAGED_PLAN_PHASE_INSTRUCTION` | 规划相说明/指令。 |
| `AGENT_STAGED_PLAN_ALLOW_NO_TASK` | 是否允许无任务跳过执行。 |
| `AGENT_STAGED_PLAN_FEEDBACK_MODE` | `fail_fast` / `patch_planner`。 |
| `AGENT_STAGED_PLAN_PATCH_MAX_ATTEMPTS` | `patch_planner` 补丁轮上限。 |
| `AGENT_STAGED_PLAN_ENSEMBLE_COUNT` | 逻辑多规划员份数（1–3，默认 1）。 |
| `AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM` | CLI / `chat` 无工具规划轮是否向 stdout 打印模型流（默认 `true`；见下文「分阶段规划」）。 |
| `AGENT_STAGED_PLAN_OPTIMIZER_ROUND` | 是否启用规划步骤优化轮（默认 `true`）。 |

### 整请求 Chrome Trace（`run_agent_turn`）

| 环境变量 | 说明 |
| --- | --- |
| `CRABMATE_REQUEST_CHROME_TRACE_DIR` | 非空目录时，每轮 **`run_agent_turn`**（Web `/chat*`、CLI `chat`/`repl` 等）结束写入 **`turn-{unix_ms}.json`**（Chrome Trace Event Format，`displayTimeUnit: us`）。在 **async 主路径**记录 **`llm.chat_completions`**（每次 `complete_chat_retrying`）与 **`agent.tools_batch`**（每批工具调度）的 **B/E** 区间；**`spawn_blocking` 内耗时**不在此文件内展开。 |

### 工作流（Chrome Trace 导出）

| 环境变量 | 说明 |
| --- | --- |
| `CRABMATE_WORKFLOW_CHROME_TRACE_DIR` | 设为非空目录时，每次 **`workflow_execute` DAG 实际执行结束**后，将本次 **`trace`** 写成 Chrome **Trace Event Format** JSON（`workflow-{run_id}-{unix_ms}.json`），可用 `chrome://tracing` 或 [Perfetto UI](https://ui.perfetto.dev/) 打开。文件内含 **`displayTimeUnit: us`**（`ts`/`dur` 为微秒，时间轴相对首条 trace 事件）。**`trace`** 含 **`node_run_start` / `node_run_end`**（整节点墙钟，含审批与重试）、**`node_attempt_*`**、失败补偿时的 **`compensation_phase_*`**；节点事件带 **`tool_name`**、**`phase`**（`main` / `compensation`）。成功写入时结果 JSON 另含 **`chrome_trace_path`**（生成文件路径）。写入失败仅打日志，不影响工具返回。**若同时设置 `CRABMATE_REQUEST_CHROME_TRACE_DIR`**：工作流事件**并入**对应回合的 **`turn-*.json`**，**不再**生成独立 `workflow-*.json`，且 **`chrome_trace_path` 为 null**。 |
| `AGENT_WORKFLOW_CHROME_TRACE_DIR` | 与上一项同义（`AGENT_*` 别名）。 |

### 队列、并行与缓存

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_CHAT_QUEUE_MAX_CONCURRENT` | 对话队列最大并发。 |
| `AGENT_CHAT_QUEUE_MAX_PENDING` | 对话队列最大排队。 |
| `AGENT_PARALLEL_READONLY_TOOLS_MAX` | 同轮只读工具并行上限。 |
| `AGENT_READ_FILE_TURN_CACHE_MAX_ENTRIES` | 单轮 `read_file` 缓存条目；`0` 关闭；写类工具或工作区变更后整表清空。 |
| `AGENT_TEST_RESULT_CACHE_ENABLED` | 测试输出进程内 LRU 是否启用。 |
| `AGENT_TEST_RESULT_CACHE_MAX_ENTRIES` | LRU 容量。对 `cargo_test`、`rust_test_one`、`npm_run`（`script` 为 `test`）、以及 `run_command` 的 `cargo`+`test`（参数**不得**含 `--nocapture` / `--test-threads`）按指纹复用上次截断输出，首行标注 **`[CrabMate 测试输出缓存命中]`**；不跨重启；指纹不含 `RUST_TEST_THREADS` 等——依赖环境一致性时请关闭。 |

### 会话工作区变更集

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_SESSION_WORKSPACE_CHANGELIST_ENABLED` | 是否注入 `crabmate_workspace_changelist` user 条。 |
| `AGENT_SESSION_WORKSPACE_CHANGELIST_MAX_CHARS` | 注入正文上限。默认按 `long_term_memory_scope_id`（Web 为 `conversation_id`；CLI 无记忆时为 `__default__`）累积写路径与 unified diff；不写入会话 SQLite（保存前剥离）。**`workflow_execute` 节点内工具**不汇入此表。 |

### 命令白名单、MCP、会话存储

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_ALLOWED_COMMANDS` | `run_command` 白名单，逗号分隔。嵌入默认另含 **`docker`**、**`podman`**、**`mvn`**、**`gradle`**（供内置 JVM/容器工具与手动 `run_command`）；完整列表见 **`config/tools.toml`**。 |
| `AGENT_MCP_ENABLED` | 是否启用 MCP。 |
| `AGENT_MCP_COMMAND` | MCP stdio 启动命令。 |
| `AGENT_MCP_TOOL_TIMEOUT_SECS` | MCP 工具超时（秒）。同一进程按指纹复用 stdio；**`crabmate mcp list`** 不要求 `API_KEY`；**`mcp list --probe`** 会启动子进程。 |
| `AGENT_CODEBASE_SEMANTIC_SEARCH_ENABLED` | 是否为模型注册 **`codebase_semantic_search`** 工具（`false` 时从当轮工具列表移除）。 |
| `AGENT_CODEBASE_SEMANTIC_INVALIDATE_ON_WORKSPACE_CHANGE` | 写工具成功或本轮 **`workspace_changed`** 后是否删除语义索引中相关行（`false` 关闭；`run_command` 等仍整表清空以防漏删）。 |
| `AGENT_CODEBASE_SEMANTIC_INDEX_SQLITE_PATH` | 相对工作区的语义索引 SQLite 路径；空则默认 **`.crabmate/codebase_semantic.sqlite`**（须仍在工作区内）。 |
| `AGENT_CODEBASE_SEMANTIC_MAX_FILE_BYTES` | 参与索引的单文件最大字节数。 |
| `AGENT_CODEBASE_SEMANTIC_CHUNK_MAX_CHARS` | 嵌入分块最大字符数。 |
| `AGENT_CODEBASE_SEMANTIC_TOP_K` | 检索默认 Top-K（工具参数可覆盖）。 |
| `AGENT_CODEBASE_SEMANTIC_QUERY_MAX_CHUNKS` | 单次 **`query`** 最多扫描多少个向量块（默认 **50000**；**0** 表示不限制，大索引慎用）；工具参数 **`query_max_chunks`** 可覆盖。 |
| `AGENT_CODEBASE_SEMANTIC_REBUILD_MAX_FILES` | **`rebuild_index`** 时最多索引文件数（防超大仓）。 |
| `AGENT_CONVERSATION_STORE_SQLITE_PATH` | 会话 SQLite 路径。 |

### 首轮注入（备忘、画像、依赖摘要）

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_MEMORY_FILE_ENABLED` | 工作区备忘文件注入。 |
| `AGENT_MEMORY_FILE` | 备忘文件路径。 |
| `AGENT_MEMORY_FILE_MAX_CHARS` | 备忘最大字符。 |
| `AGENT_PROJECT_PROFILE_INJECT_ENABLED` | 项目画像注入。 |
| `AGENT_PROJECT_PROFILE_INJECT_MAX_CHARS` | 画像最大字符。 |
| `AGENT_PROJECT_DEPENDENCY_BRIEF_INJECT_ENABLED` | 依赖结构摘要（与画像/备忘合并为一条 `user`）。 |
| `AGENT_PROJECT_DEPENDENCY_BRIEF_INJECT_MAX_CHARS` | 由 `cargo metadata`（workspace resolve 边 + Mermaid）与根/`frontend` 的 `package.json` 依赖名节选组成；不含版本与 lockfile 全文；`0` 关闭该段。 |

### 工具解释卡

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_TOOL_CALL_EXPLAIN_ENABLED` | 非只读工具是否要求 `crabmate_explain_why`。 |
| `AGENT_TOOL_CALL_EXPLAIN_MIN_CHARS` | 解释最短长度。 |
| `AGENT_TOOL_CALL_EXPLAIN_MAX_CHARS` | 解释最长长度。 |

### 长期记忆

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_LONG_TERM_MEMORY_ENABLED` | 是否启用长期记忆。 |
| `AGENT_LONG_TERM_MEMORY_SCOPE_MODE` | 作用域模式。 |
| `AGENT_LONG_TERM_MEMORY_VECTOR_BACKEND` | 默认 `fastembed`，可 `disabled`。 |
| `AGENT_LONG_TERM_MEMORY_STORE_SQLITE_PATH` | 记忆向量/元数据 SQLite。 |
| `AGENT_LONG_TERM_MEMORY_TOP_K` | 检索 Top-K。 |
| `AGENT_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK` | 分块最大字符。 |
| `AGENT_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX` | 索引最小字符阈值。 |
| `AGENT_LONG_TERM_MEMORY_ASYNC_INDEX` | 是否异步索引。 |
| `AGENT_LONG_TERM_MEMORY_MAX_ENTRIES` | 条目上限。 |
| `AGENT_LONG_TERM_MEMORY_INJECT_MAX_CHARS` | 注入模型上下文的最大字符。 |

Web 已配置 `conversation_store_sqlite_path` 时会话库与长期记忆可共用同一 SQLite；纯内存会话须单独配置 `long_term_memory_store_sqlite_path` 才能持久化。CLI 默认路径为 `run_command_working_dir/.crabmate/long_term_memory.db`。若在 **repl / chat** 下启用但打开库失败，进程会向 **stderr** 打印一次性警告并继续运行（本进程内不注入记忆）；另有 `crabmate` 目标下的 `warn` 日志。

### 联网搜索与 `http_fetch`

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_WEB_SEARCH_PROVIDER` | 搜索供应商。 |
| `AGENT_WEB_SEARCH_API_KEY` | 搜索 API 密钥。 |
| `AGENT_WEB_SEARCH_TIMEOUT_SECS` | 超时（秒）。 |
| `AGENT_WEB_SEARCH_MAX_RESULTS` | 最大结果数。 |
| `AGENT_HTTP_FETCH_ALLOWED_PREFIXES` | 允许 URL 前缀。 |
| `AGENT_HTTP_FETCH_TIMEOUT_SECS` | 抓取超时（秒）。 |
| `AGENT_HTTP_FETCH_MAX_RESPONSE_BYTES` | 响应体截断上限。 |

**`http_fetch` / `http_request` 外圈超时**：除 **`http_fetch_timeout_secs`**（`reqwest` 读超时）外，异步路径在 **`spawn_blocking`** 外包一层 **`tokio::time::timeout`**；默认与 **`command_timeout_secs`**、**`http_fetch_timeout_secs`** 取较大者。可在 TOML **`[tool_registry]`** 中设 **`http_fetch_wall_timeout_secs`** / **`http_request_wall_timeout_secs`** 单独收紧或放宽（见 **`config/tools.toml` 文件末尾注释**）。

### `tool_registry` 策略（`tools.toml` / 主配置）

在 **`config/tools.toml`** 或与嵌入顺序一致的用户 **`config.toml`** 中增加可选表 **`[tool_registry]`**，与 **`[agent]`** 一并解析并合并进 **`AgentConfig`**（热重载随 **`apply_hot_reload_config_subset`** 更新）。用于运维统一调参，**无对应 `AGENT_*` 环境变量**（须写 TOML）。

| 键 | 说明 |
| --- | --- |
| **`http_fetch_wall_timeout_secs`** | **`http_fetch`** 外圈 `tokio::time::timeout`（秒）。 |
| **`http_request_wall_timeout_secs`** | **`http_request`** 外圈超时；省略则与 fetch 外圈逻辑一致。 |
| **`parallel_wall_timeout_secs`** | 子表：按 **`ToolExecutionClass` 蛇形键**覆盖并行只读批与 **`SyncDefault`+`spawn_blocking`** 墙上时钟，例如 **`blocking_sync`**、**`http_fetch_spawn_timeout`**、**`weather_spawn_timeout`** 等。 |
| **`parallel_sync_denied_tools`** | 禁止与其它只读工具同批并行的工具名（精确匹配）；省略用内建构建锁类规则。 |
| **`parallel_sync_denied_prefixes`** | 同上，按工具名前缀拒绝并行批。 |
| **`sync_default_inline_tools`** | 在当前 async 任务上**内联**执行的 SyncDefault 工具（跳过 **`spawn_blocking`**）；省略则仅内建轻量工具。 |
| **`write_effect_tools`** | 视为非只读（写副作用）的工具名；影响 **`is_readonly_tool`**、解释卡、代码语义索引失效等；省略用内建写工具表。 |

### 上下文与工具消息

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_MAX_MESSAGE_HISTORY` | 保留消息条数上限。 |
| `AGENT_TOOL_MESSAGE_MAX_CHARS` | 单条 `role: tool` 发往模型前压缩阈值。 |
| `AGENT_TOOL_RESULT_ENVELOPE_V1` | `crabmate_tool` 信封 v1。 |
| `AGENT_MATERIALIZE_DEEPSEEK_DSML_TOOL_CALLS` | DeepSeek DSML 工具调用物化。 |
| `AGENT_CONTEXT_CHAR_BUDGET` | 上下文字符预算。 |
| `AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM` | 摘要后至少保留条数。 |
| `AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS` | 触发摘要的字符阈值。 |
| `AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES` | 摘要保留尾部消息数。 |
| `AGENT_CONTEXT_SUMMARY_MAX_TOKENS` | 摘要请求 max_tokens。 |
| `AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS` | 摘要转写最大字符。 |

### CLI

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_TUI_LOAD_SESSION_ON_START` | 启动时是否从磁盘恢复会话。 |
| `AGENT_TUI_SESSION_MAX_MESSAGES` | 会话文件最大消息数。 |
| `AGENT_REPL_INITIAL_WORKSPACE_MESSAGES_ENABLED` | `true` 时后台构建 `initial_workspace_messages`（画像、依赖摘要，并尊重 `tui_load_session_on_start`）；默认 `false`。TOML：`[agent] repl_initial_workspace_messages_enabled`。 |
| `AGENT_CLI_WAIT_SPINNER` | 非空且非 `0`/`false` 时，交互式 CLI 与 **`chat`** 纯文本流式在首包前于 stderr 显示 indicatif spinner；**`NO_COLOR`** 或 stderr 非 TTY 时不启用。 |

### Docker 工具沙盒

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_MODE` | `none` \| `docker`。 |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE` | `docker` 模式镜像（必填）。 |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK` | 空为 `none` 网络；如 `bridge` 以使出网工具可用。 |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS` | 单次容器等待上限（秒）。 |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER` | Docker `Config.user`；`current`/`host` 等语义见下文「SyncDefault 工具 Docker 沙盒」。 |

连接 Docker 守护进程时亦可使用非 `AGENT_` 的 **`DOCKER_HOST`**（与 `docker` CLI / bollard 一致）。

```bash
export AGENT_MODEL=deepseek-reasoner
cargo run
```

## 本地 Ollama（OpenAI 兼容）

Ollama 提供 **`http://127.0.0.1:11434/v1`** 下的 OpenAI 兼容 API。建议配置：

```toml
[agent]
api_base = "http://127.0.0.1:11434/v1"
model = "llama3.2"   # 以 ollama list 为准
llm_http_auth_mode = "none"
```

然后可不设环境变量 **`API_KEY`** 即启动 `serve` / `repl` / `chat`。**工具调用（function calling）**依赖模型与 Ollama 版本；若不稳定可先 **`--no-tools`** 验证对话。`crabmate config`（自检）**不要求** **`API_KEY`**。

## MiniMax（OpenAI 兼容）

MiniMax 提供 **`https://api.minimaxi.com/v1`**（与官方文档一致；亦可能见 **`https://api.minimax.io/v1`** 等别名，以控制台为准）下的 OpenAI 兼容 **`POST …/chat/completions`**。官方文档示例含 **`role: "system"`**（见 [OpenAI API 兼容](https://platform.minimaxi.com/docs/api-reference/text-openai-api)），但**线上接口仍常返回** HTTP 400 **`invalid message role: system`**。CrabMate **嵌入默认 `config/default_config.toml`** 以 **DeepSeek** 等常规调用为主，将 **`llm_fold_system_into_user`** 默认设为 **`false`**（保留独立 **`system`** 条）。接 **MiniMax** 时**建议**显式设 **`llm_fold_system_into_user = true`**：出站请求把系统提示**并入**第一条相关 **`user`**，语义与「首条 system + user」等价，一般可消除该错误。若你确认所用网关**接受**独立 **`system`** 条，可保持 **`false`**。

**本仓库已实测的 `model` 示例**（与 CrabMate OpenAI 兼容调用链联调）：**`MiniMax-M2.7`**、**`MiniMax-M2.7-highspeed`**、**`MiniMax-M2.5`**。更多模型名与能力以 MiniMax 控制台及官方 API 文档为准。

建议配置：

```toml
[agent]
api_base = "https://api.minimaxi.com/v1"
model = "MiniMax-M2.7"   # 或 M2.7-highspeed / M2.5 等；以控制台为准
llm_http_auth_mode = "bearer"
llm_fold_system_into_user = true
# llm_reasoning_split：可省略；未写时 MiniMax 网关默认为 true（思维链分离）
# llm_reasoning_split = false   # 若不需要 reasoning_split，可显式关闭
```

环境变量 **`API_KEY`** 填平台发放的密钥（与 DeepSeek 等一致，走 **`Authorization: Bearer`**）。**`llm_reasoning_split`** 为 **`true`**（含未写配置时 MiniMax 的默认值）时，请求体会包含 **`reasoning_split: true`**（与文档中 `extra_body={"reasoning_split": True}` 一致）；供应商若在流式 **`delta`** 中返回 **`reasoning_details`**（常见为带 **`text`** 的 JSON 数组），CrabMate 会将其**增量合并**进内部的 **`reasoning_content`** 流与终态消息，终端/Web 仍按现有「思考 / 正文」路径展示。非 MiniMax 网关未写该键时默认为 **`false`**；MiniMax 下不需要分离思维链时请显式 **`llm_reasoning_split = false`** 或 **`AGENT_LLM_REASONING_SPLIT=0`**。

## 智谱 GLM（OpenAI 兼容）

智谱在 **`https://open.bigmodel.cn/api/paas/v4`** 下提供 OpenAI 兼容 **`POST …/chat/completions`**。请将 **`api_base`** 设为 **`https://open.bigmodel.cn/api/paas/v4`**（**不要**再拼 `/chat/completions`，程序会追加），**`model`** 如 **`glm-5`**；环境变量 **`API_KEY`** 填控制台密钥（**`llm_http_auth_mode = bearer`**），请求头 **`Authorization: Bearer …`** 与官方 cURL 一致。

### 与最小 cURL 对齐（默认流式、无深度思考）

下列官方风格的请求只需 **`model`**、**`messages`**、**`stream: true`**（无 **`thinking`**）即可工作：

```bash
curl --location 'https://open.bigmodel.cn/api/paas/v4/chat/completions' \
  --header 'Authorization: Bearer YOUR_API_KEY' \
  --header 'Content-Type: application/json' \
  --data '{
    "model": "glm-5",
    "messages": [{ "role": "user", "content": "写一首关于春天的诗" }],
    "stream": true
}'
```

在 CrabMate 中：**默认 `llm_bigmodel_thinking = false`** 时出站 JSON **不**含 **`thinking`**，与上例同形（另会带 OpenAI 兼容常用字段 **`max_tokens`**、**`temperature`** 等，来自 **`[agent]`** 配置；智谱兼容端一般接受）。Web / 默认 CLI 对话为 **SSE 流式**，对应 **`stream: true`**；若使用 **`--no-stream`** / **`no_stream`**，则对应 **`stream: false`**。

### 可选：GLM-5 深度思考（`thinking`）

文档中的 **深度思考** 为 **`thinking: { "type": "enabled" }`**（见 [GLM-5 调用示例](https://docs.bigmodel.cn/cn/guide/models/text/glm-5)）。需要时设 **`llm_bigmodel_thinking = true`**（或 **`AGENT_LLM_BIGMODEL_THINKING=1`**）。流式下思维链可走 **`delta.reasoning_content`**，与现有解析路径一致。

### 配置示例

**最小对接（与上节 cURL 一致，不含 `thinking`）：**

```toml
[agent]
api_base = "https://open.bigmodel.cn/api/paas/v4"
model = "glm-5"
llm_http_auth_mode = "bearer"
# llm_bigmodel_thinking 默认 false，可不写
```

**启用文档中的深度思考时：**

```toml
[agent]
api_base = "https://open.bigmodel.cn/api/paas/v4"
model = "glm-5"
llm_http_auth_mode = "bearer"
llm_bigmodel_thinking = true
# max_tokens、temperature 等按控制台与用量自行调整
```

## Moonshot（Kimi，OpenAI 兼容）

Moonshot 在 **`https://api.moonshot.cn`** 提供与 OpenAI SDK 兼容的 HTTP API；单轮对话示例见 [Kimi Chat API / 单轮对话](https://platform.moonshot.cn/docs/api/chat#%E5%8D%95%E8%BD%AE%E5%AF%B9%E8%AF%9D)。

- **请求地址**：**`POST https://api.moonshot.cn/v1/chat/completions`**（与文档一致）。
- **CrabMate**：将 **`api_base`** 设为 **`https://api.moonshot.cn/v1`**（程序会再追加 **`chat/completions`**）。**`model`** 填文档中的模型 ID（如 **`kimi-k2.5`**、**`kimi-k2-thinking`**、**`moonshot-v1-8k`** 等，以 [List Models](https://platform.moonshot.cn/docs/api/chat#list-models) 与控制台为准）。环境变量 **`API_KEY`** 填平台密钥（**`llm_http_auth_mode = bearer`**）。

**`max_tokens` 与 `max_completion_tokens`**：Kimi 文档将 **`max_tokens`** 标为可选且**已废弃**，推荐使用 **`max_completion_tokens`** 表示**完成段**上限。CrabMate 当前仍序列化 OpenAI 常见字段 **`max_tokens`**（来自 **`[agent] max_tokens`**），多数兼容网关会接受；若遇 **`invalid_request_error`** 与长度相关，请核对文档中的 **`max_completion_tokens`** 语义并适当调低 **`max_tokens`** 或关注后续版本是否增加该字段。

**`thinking`（仅 kimi-k2.5）**：文档说明可选 **`thinking`**，取值 **`{"type": "enabled"}`** 或 **`{"type": "disabled"}`**，**服务端默认接近 enabled**。不写入请求体时由 Kimi 默认行为决定。若需**显式关闭** k2.5 思考模式，在 CrabMate 中设 **`llm_kimi_thinking_disabled = true`**（或 **`AGENT_LLM_KIMI_THINKING_DISABLED=1`**）；实现上**仅当**当前 **`model`** 为 **`kimi-k2.5` / `kimi-k2.5-…`** 时才会在 JSON 中带 **`thinking: { "type": "disabled" }`**，避免误发给其它网关。若同时开启 **`llm_bigmodel_thinking`** 且 **`model`** 为 k2.5 系列，**以 Kimi `disabled` 优先**（先写 Kimi 关闭）。

**多轮与工具调用**：在 **kimi-k2.5** 且**未**关闭思考（默认）时，接口会校验历史里带 **`tool_calls`** 的 assistant 消息必须带 **`reasoning_content`**，否则会报类似 **`thinking is enabled but reasoning_content is missing in assistant tool call message`**。CrabMate 对此类消息在出站时**保留**会话中的 **`reasoning_content`**（若上游当时未返回则补空串），其它 assistant 条仍按惯例剥离思维链以省 token。关闭思考后按 Kimi 行为一般不再强制该字段。

**`temperature` / `top_p`**：文档对各系列默认值不同，且部分模型**不可修改**。CrabMate 按 **`model`** ID 自动钳制出站 **`temperature`**（含 Web 单条覆盖、上下文摘要轮等），避免 **`invalid temperature`**：**`kimi-k2.5` / `kimi-k2.5-…`** 与 **`kimi-k2-thinking` / `kimi-k2-thinking-…`** → **`1.0`**；其它 **`kimi-k2` / `kimi-k2-…`**（如 **`kimi-k2-0905-preview`**）→ **`0.6`**。**`moonshot-v1-*`** 等其它 Kimi 模型仍使用配置中的 **`temperature`**；若遇类似报错请查阅当前模型说明。

配置示例：

```toml
[agent]
api_base = "https://api.moonshot.cn/v1"
model = "kimi-k2.5"
llm_http_auth_mode = "bearer"
# llm_kimi_thinking_disabled = true   # 可选：关闭 k2.5 默认思考
```

## 配置文件示例

```toml
[agent]
api_base = "https://api.deepseek.com/v1"
model = "deepseek-reasoner"
# system_prompt = "…"   # 仅写此项时会取消默认的 system_prompt_file，改为内联
# system_prompt_file = "my_prompt.txt"   # 相对路径按「系统提示词」一节解析
# cursor_rules_enabled = true
# cursor_rules_dir = ".cursor/rules"
```

## 终答规划（`final_plan_requirement`）

控制模型以**非 tool_calls** 结束一轮时，是否必须嵌入可解析的 `agent_reply_plan` JSON（详见 **`docs/DEVELOPMENT.md`**）。

- **`workflow_reflection`**（默认）：仅在工作流反思后要求规划。
- **`never`**：关闭该校验。
- **`always`**（实验性）：每次终答都校验，**调用次数与费用明显更高**；适合强合规或调试。

若存在 `workflow_validate_only` 结果，服务端还会按 `spec.layer_count` 约束规划步骤条数。若规划步骤填写了可选字段 `workflow_node_id`，其值须属于该次（或最近一次）`workflow_execute` 工具结果中 `nodes[].id`。

## 规划重写（`plan_rewrite_max_attempts`）

规划不合格时追加「请重写」的上限；超过后流式前端可能收到 `code: plan_rewrite_exhausted`。

## 逻辑双 agent（`planner_executor_mode = logical_dual_agent`）

先无工具规划轮，再执行器循环；planner 上下文会过滤 `role: tool` 正文。与 `staged_plan_execution` 并存时本模式优先。

## 分阶段规划（`staged_plan_execution`）

在 `planner_executor_mode = single_agent` 且开启时，每条用户消息先走无工具规划轮，再按 `steps` 执行。`no_task` + 空 `steps` 可跳过执行。规划 JSON 无法解析时降级为常规工具循环。API 调用通常多于关闭时。

**步级反馈（`staged_plan_feedback_mode`）**：默认 `fail_fast`（某步子循环 `Err` 或步内存在失败工具结果时，整轮计划按失败结束）。设为 `patch_planner` 时，会向规划器注入简短反馈并无工具重跑规划轮，将补丁 `steps` 与「当前步及之后」合并后继续执行（受 `staged_plan_patch_max_attempts` 限制，多耗 API）。

**CLI 规划轮终端输出（`staged_plan_cli_show_planner_stream`，默认 `true`，环境变量 `AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM`）**：仅影响 **CLI / `chat` 等 `out: None` 路径** 下，**无工具规划轮**与 **`patch_planner` 补丁规划轮**是否向 stdout 流式或整段打印模型原文（`Agent:` 前缀及正文）。设为 `false` 时这些轮次不在终端打印模型输出，仍保留 `staged_plan_notice` 队列摘要、分步注入 user 转录与后续执行步的助手输出；Web SSE 路径不受影响。

**规划步骤优化轮（`staged_plan_optimizer_round`，默认 `true`，环境变量 `AGENT_STAGED_PLAN_OPTIMIZER_ROUND`）**：在首轮 `agent_reply_plan` v1 解析成功且 `steps` 不少于 2 时，再追加一轮无工具请求，请模型合并**无数据依赖**的只读探查步，并提示在同一执行步内对「可同轮并行批处理」的内建工具（与执行层 `parallel_readonly_tools` 判定一致，不限于 `read_file`）发起多次调用。解析失败或用户取消优化轮时沿用首轮规划；成功则追加优化轮 assistant 并采用新 `steps`（多一次 API）。

**逻辑多规划员与合并（`staged_plan_ensemble_count`，默认 `1`，环境变量 `AGENT_STAGED_PLAN_ENSEMBLE_COUNT`，合法值钳制为 1–3）**：`1` 表示关闭。为 `2` 或 `3` 时，在首轮规划写入历史后，再**串行**发起 1 或 2 次无工具「独立规划员」请求（通过服务端注入的 user 正文区分角色；**辅助规划员的 assistant 不写入会话历史**，仅合并轮的 user+assistant 会保留），最后追加一轮「合并多份草案」的无工具请求，产出单一 `steps` 后再进入上述步骤优化轮（若启用）。仍为**同一进程、同一模型与密钥**；不保证质量更优，且 **API 次数与费用明显增加**（例如 `3` + 优化轮 ≈ 首轮外再多 3 次规划类调用）。某辅助轮解析失败时停止追加规划员；若最终有效草案不足 2 份则不跑合并轮。

## SyncDefault 工具 Docker 沙盒（`sync_default_tool_sandbox_mode`）

### 模式与覆盖范围

- **`none`（默认）**：与历史一致，在 Agent 进程内 `spawn_blocking` 执行 `HandlerId::SyncDefault` 工具；**`run_command` 等**也在宿主执行。
- **`docker`**：**SyncDefault** 以及 **`run_command` / `run_executable` / `get_weather` / `web_search` / `http_fetch` / `http_request`** 在宿主完成白名单与审批（若有）后，每次调用经 **[bollard](https://docs.rs/bollard)** 走 **Docker Engine HTTP API** 创建并运行一次性容器（等价于 `docker run --rm -i`）：挂载当前工作区到容器内 **`/workspace`**（读写），只读挂载**当前正在运行的宿主 `crabmate` 可执行文件**到 **`/crabmate`**，在容器内执行 **`crabmate tool-runner-internal`**（由服务端生成临时 JSON 配置并只读挂入容器）。**Linux/macOS** 默认连接本地 Unix 套接字（与 `docker` CLI 相同）；**`DOCKER_HOST`** 在部分环境下亦可由 bollard 解析。
- **不进入沙盒**：**`workflow_execute`**、**MCP 代理工具**（`mcp__*`）仍只在宿主执行。

### 使用前准备

1. **Docker 守护进程可用**：本机能 `docker ps` 或等价 API 访问（与 CLI 同源套接字或 `DOCKER_HOST`）。
2. **架构一致**：宿主 **`crabmate` 二进制**与容器 **CPU 架构**须一致（例如宿主为 `linux/amd64` 则镜像也应为 `amd64`）。实现上**不会**在镜像内自带 crabmate，而是**挂载宿主二进制**；若你改为在镜像内安装 crabmate，则须自行保证版本与调用方式一致（非默认路径，需改镜像/入口，本仓库不维护该方案）。
3. **镜像职责**：镜像提供 **OS + 工具依赖**（`git`、`rg`、`cargo`、`python3`、`npm`、`bc`、`clang-format` 等——按你在工作区里**实际会调用的内置工具**安装；仓库**不提供**固定发布的「官方工具镜像」，`config/sandbox.toml` 中的 `your-registry/crabmate-tools:latest` 仅为占位。

### 镜像与最小示例

- **`sync_default_tool_sandbox_docker_image`**：`docker` 模式**必填**（`finalize` 时非空校验）。任选满足依赖的镜像名（自建或私有 registry 均可）。
- 最小思路：以 **`debian:bookworm-slim`**（或 **`ubuntu:22.04`** 等）为基础，`apt-get install` 你需要的 CLI；Rust 项目可再装 **`build-essential`**、**`pkg-config`**、**`libssl-dev`** 等。示例 Dockerfile（按需增删包）：

```dockerfile
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates git ripgrep curl \
  && rm -rf /var/lib/apt/lists/*
# 若需 cargo / node / bc 等，继续 apt 或复制多阶段构建产物
```

构建并推送后，在配置中填写例如 `sync_default_tool_sandbox_docker_image = "your-registry/crabmate-tools:dev"`。

### 启用步骤（配置）

在 **`config.toml`** 的 **`[agent]`** 段（或环境变量）中设置，例如：

```toml
[agent]
sync_default_tool_sandbox_mode = "docker"
sync_default_tool_sandbox_docker_image = "your-registry/crabmate-tools:dev"
# 需要天气 / 联网搜索 / HTTP 工具出网时改为 bridge（或你环境可用的网络名）
# sync_default_tool_sandbox_docker_network = "bridge"
# sync_default_tool_sandbox_docker_timeout_secs = 600
# sync_default_tool_sandbox_docker_user = "current"   # Unix 默认等效：当前 euid:egid
```

或使用环境变量（覆盖 TOML）：`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_MODE=docker`、`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE=...` 等。

### 网络

- **`sync_default_tool_sandbox_docker_network` 为空**：容器使用 **`network_mode: none`**，**无出网**；适合仅本地读写、`read_file`、`run_command` 白名单内离线命令等。
- **非空**（如 **`bridge`**）：容器加入该 Docker 网络，**`get_weather` / `web_search` / `http_fetch` / `http_request`** 等才可访问外网；请按环境选择，避免在不可信工作区与宽松网络组合下放大风险。

### 超时与用户

- **`sync_default_tool_sandbox_docker_timeout_secs`**：单次容器生命周期等待上限（秒，默认 600），超时后 **force remove** 容器。
- **`sync_default_tool_sandbox_docker_user`**：写入 Docker **`Config.user`**（等价 `docker run --user`）。**默认**（配置键省略或空、或 **`current` / `host`**）：在 **Unix** 上使用**当前进程有效** **`uid:gid`**（`geteuid` / `getegid`），减轻 bind mount 工作区产生 root 拥有文件的常见问题；**非 Unix** 上省略 `user`（与 `image` 相同）。**`image` 或 `default`**：不设置，沿用镜像 **`USER`**（常为 root）。其它值：原样传给 Docker（如 `1000:1000`、`myuser` 等，须与镜像内账户/权限一致）。

### 安全与运维提示

- **临时配置 JSON**：每次工具调用会在宿主 **`TMPDIR`** 写入 runner 配置（Unix 尝试 **`0600`**），其中可能含 **`web_search_api_key`** 等；仅在**可信主机**上使用，并注意磁盘与备份策略。
- **沙盒边界**：宿主仍负责 **命令白名单、HTTP 前缀、Web/CLI 审批**；Docker 隔离的是**执行环境**，不替代策略配置。
- **性能**：每次工具调用起停容器，延迟与 Docker 开销高于 `none` 模式。

环境变量：`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_MODE`、`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE`、`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK`、`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS`、`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER`。

## 系统提示词

- **默认**：嵌入的 **`config/default_config.toml`** 使用 **`system_prompt_file = "config/prompts/default_system_prompt.md"`**，运行时读盘，**修改该 Markdown 无需重新编译**。
- **相对路径解析顺序**：进程**当前工作目录** → 各层**覆盖配置文件所在目录**（后加载的优先，如 `.agent_demo.toml` 先于 `config.toml`）→ **`run_command_working_dir`**（已规范化的工作区根）。**绝对路径**仅尝试该路径。
- **覆盖与优先级**：若某层 TOML **只写**内联 **`system_prompt`**、**不写**该层的 `system_prompt_file`，则会**取消**继承自更早层的 `system_prompt_file`，改为使用内联。环境变量阶段：**`AGENT_SYSTEM_PROMPT`** 会清除已合并的 `system_prompt_file`；随后若存在 **`AGENT_SYSTEM_PROMPT_FILE`** 则再设为文件路径（两者同时设置时以文件为准）。
- **finalize 阶段**：若仍存在 `system_prompt_file` 则读文件；否则使用非空内联；二者皆无则报错。

仓库内默认正文含工具与任务拆分等约定（例如**同一工作区路径在未被修改前不要重复 `read_file`**）。完全自定义时可改 `config/prompts/default_system_prompt.md` 或换用自有路径。

## 多角色（agent_roles）

在全局 `system_prompt` 之外，可为**命名 id** 配置不同的首条 `system` 正文（每条在加载时**同样**经 `cursor_rules_*` 合并，与全局一致）。

- **定义方式（二选一或混用，后加载覆盖同 id 字段）**  
  1. 主配置文件中的 **`[[agent_roles]]`** 表数组：每行含 **`id`**，以及 **`system_prompt`** 和/或 **`system_prompt_file`**（其一即可；`system_prompt` 为空字符串表示**沿用**全局合并后的 `system_prompt`）。  
  2. 仓库根 **`config/agent_roles.toml`**（未使用 **`--config`** 时）；若使用 **`crabmate --config path/to/foo.toml`**，则读取 **`path/to/agent_roles.toml`**（与主配置**同目录**）。文件形态为 **`[agent_roles]`** + **`default_role`** + **`[agent_roles.roles.<id>]`** 子表（见 `config/agent_roles.toml` 注释示例）。
- **默认角色**：`[agent]` 中 **`default_agent_role`**，或 `agent_roles.toml` 的 **`[agent_roles] default_role`**，或环境变量 **`AGENT_DEFAULT_AGENT_ROLE`**。须指向已定义的角色 id；未配置默认时，未显式选角则使用全局 **`system_prompt`**。
- **Web**：`POST /chat`、`POST /chat/stream` 可选 JSON 字段 **`agent_role`**（与 `conversation_id` 同类字符集，最长 64）。**仅当**服务端**尚无**该 `conversation_id` 的会话历史时生效（首轮建立 `system`）；已有会话时忽略，避免中途改口与人格不一致。
- **CLI**：全局 **`--agent-role <id>`**（`repl` / `chat` 等）。与 **`--system-prompt-file`** 互斥。`chat` 在**未**使用 **`--messages-json-file`** 时，该 id 用于构造首条 system（含 **`--message-file`** 首轮）。
- **REPL**：**`/agent list`** 先列出内建伪 id **`default`**（未显式选用命名角色；语义同 Web 未传 **`agent_role`**：有 **`default_agent_role_id`** 则用其条目，否则用全局 **`system_prompt`**），再列配置中的命名 id（与 **`GET /status`** 的 **`agent_role_ids`** 同源）；**`/agent set default`**（不区分大小写）清除本进程 REPL 的显式角色并按新 system **重建首轮消息**。
- **热重载**：**`POST /config/reload`** / **`/config reload`** 会重载角色表（与 `system_prompt` 一致）。
- **`GET /status`**：返回 **`agent_role_ids`**（升序）、**`default_agent_role_id`**，供前端展示角色下拉等。

## Cursor-like 规则注入

`cursor_rules_enabled` 为真时读取 `cursor_rules_dir` 下 `*.mdc`（可附加 `AGENTS.md`），拼到系统提示词末尾，长度受 `cursor_rules_max_chars` 限制。

## 上下文窗口

请求前会压缩 `messages`：条数上限、`context_char_budget`、可选 LLM 摘要等。其中 **`tool_message_max_chars`**（`AGENT_TOOL_MESSAGE_MAX_CHARS`）：单条 `role: tool` 在**发往模型前**若超长则压缩；启用 **`tool_result_envelope_v1`** 时对 `crabmate_tool.output` 采用**首尾采样**并附带 `output_truncated` 等字段（见 **`docs/DEVELOPMENT.md`**）。详见 `config/tools.toml`。

## Web 对话队列（`chat_queue_*`）

`/chat` 与 `/chat/stream` 经有界队列调度；满时 **503**、`QUEUE_FULL`。`/status` 返回队列与 `per_active_jobs` 等字段。

## 只读工具并行（`parallel_readonly_tools_max`）

限制同轮多只读工具进入 blocking 池的并发数： eligible 批含内建只读 **`SyncDefault`**、**`http_fetch`**（GET/HEAD）、**`get_weather`**、**`web_search`**（不含 **`http_request`**、**`run_command`**、MCP 等）。构建锁类（如 **`cargo_*`**、**`npm_*`**）整批降级为串行。

## HTTP 客户端

进程内共享 `reqwest::Client`（连接池、Keep-Alive）。细节见 **`docs/DEVELOPMENT.md`** 中 `http_client` 说明。

## 常用模型 ID

- `deepseek-chat`（默认）
- `deepseek-reasoner`（推理链更长）
