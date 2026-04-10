**语言 / Languages:** 中文（本页）· [English](en/CONFIGURATION.md)

# 配置说明

默认配置由仓库 **`config/default_config.toml`**、**`config/session.toml`**、**`config/context_inject.toml`**、**`config/tools.toml`**、**`config/sandbox.toml`**、**`config/planning.toml`**、**`config/memory.toml`** 七段嵌入（各段主体为 **`[agent]`** 扁平键；**`config/tools.toml`** 还可选 **`[tool_registry]`** 表，见下文「`tool_registry` 策略」；**`session`** 为 CLI 会话相关 **`tui_*`** 与 **`repl_initial_workspace_messages_enabled`**；**`context_inject`** 为首轮 **`agent_memory_file_*`**、**`project_profile_inject_*`**、**`project_dependency_brief_inject_*`**；**`tools`** 的 **`[agent]`** 含 **`run_command`** 白名单/超时/工作目录、**`tool_message_*`** / **`tool_result_envelope_v1`**、**`read_file_turn_cache_*`**、**`test_result_cache_*`**、**`session_workspace_changelist_*`**、**`codebase_semantic_*`**（**`codebase_semantic_search`** 与写后失效 **`codebase_semantic_invalidate_on_workspace_change`**）、天气/搜索/**`http_fetch_*`**、**`tool_call_explain_*`**、**`mcp_*`** 等；**`sandbox`** 为 **SyncDefault Docker 沙盒** **`sync_default_tool_sandbox_*`**；**`planning`** 为规划 / 反思 / 编排；**`memory`** 为 **`long_term_memory_*`**）。`load_config` 按 **主默认 → session → context_inject → tools → sandbox → planning → memory** 顺序合并，再被 **`config.toml`** 或 **`.agent_demo.toml`** 覆盖，最后由环境变量覆盖。示例片段见 **`config.toml.example`**。

**未知键与越界数值**：用户 **`config.toml`** / **`agent_roles.toml`** 中 **`[agent]`**、**`[tool_registry]`**、**`[[agent_roles]]`** 等已声明表内若出现**未在 CrabMate 中定义的键**，TOML 解析会失败（serde **拒绝未知字段**），避免拼写错误被静默忽略。对 **`finalize` 中有上下限的数值项**（如 **`temperature`**、**`max_message_history`**、**`chat_queue_max_concurrent`** 等），若在 TOML 或 **`AGENT_*`** 中写出**超出允许范围**的值，启动（及热重载路径上的 **`load_config`**）会返回明确错误，而**不再**仅做静默截断（`clamp`）；详见源码 **`src/config/validate.rs`** 与 **`finalize`** 中的默认值说明。

## 配置热重载（无需重启 `repl` / `serve` 主进程）

- **CLI**：输入 **`/config reload`**（或 Tab 补全 **`/config reload`**）。从与启动时相同的配置文件路径（**`--config`** 或默认探测 **`config.toml`** / **`.agent_demo.toml`**）再读 TOML，并与**当前进程环境变量**合并后，将可热更字段写入内存中的 [`AgentConfig`](DEVELOPMENT.md)；随后清空 MCP 进程内 stdio 缓存，下一轮对话使用新 MCP 指纹。
- **Web**：**`POST /config/reload`**（JSON body 可为 `{}`；鉴权与 **`/chat`** 等受保护 API 一致——若启动时启用了 Web API 鉴权层则须在请求头携带 **`Authorization: Bearer <token>`** 或 **`X-API-Key: <token>`**）。成功时返回 **`{ "ok": true, "message": "…" }`**。
- **会更新的典型项**：**`api_base`**、**`model`**、**`llm_http_auth_mode`**、**`llm_reasoning_split`**、**`llm_bigmodel_thinking`**、**`llm_kimi_thinking_disabled`**、**`thinking_avoid_echo_system_prompt`**、**`thinking_avoid_echo_appendix` / `thinking_avoid_echo_appendix_file`**（附录正文解析结果）、**`temperature` / `llm_seed`**、各类**超时与重试**、**`run_command` 白名单**、**`http_fetch_allowed_prefixes`**、**`workspace_allowed_roots`**、**`web_api_bearer_token`**（仅影响 handler 内校验；见下）、**`mcp_*`**、**`[tool_registry]`**（HTTP 外圈超时、并行墙钟覆盖、并行拒绝/内联/写副作用名单）、**`system_prompt_file` 重读**、上下文与规划相关键等（实现见源码 **`apply_hot_reload_config_subset`**）。**`system`→`user` 折叠**随 **`api_base` / `model`** 热更后由下一轮请求按 MiniMax 识别自动生效（非 `AgentConfig` 字段）。
- **刻意不热更**：**`conversation_store_sqlite_path`**（会话库连接在启动时打开，改路径须重启 **`serve`**）。**`reqwest::Client`** 不重建，**`api_timeout_secs` 等**对**新连接**的生效可能受连接池保留的空闲连接影响。
- **`API_KEY`**：进程内 **`serve` / `repl` / `chat`** 启动时从**环境变量**读入并保存在 **`AppState.api_key`**（可为空）。**热重载不**重读环境里的 **`API_KEY`**。未 export 时：**Web** 须在侧栏「设置」随请求发送 **`client_llm.api_key`**；**REPL** 可用 **`/api-key set <密钥>`** 写入**本进程内存**（不写盘，**`/config reload` 不会清除**）。**`crabmate models` / `probe`** 在 **`bearer`** 下仍要求启动前环境变量 **`API_KEY`** 非空。
- **Web API 鉴权层**：若启动 **`serve`** 时 **`web_api_bearer_token` 非空**，Axum 会在该进程生命周期内挂上鉴权中间件；请求须带 **`Authorization: Bearer <同一密钥>`** 或 **`X-API-Key: <同一密钥>`**（二选一，与 Dify / Open WebUI 等常见网关习惯对齐）。热重载**不会**拆除或新增该层——**从「无 token」变为「有 token」**或反向时，须**重启 `serve`**。热重载仍会更改 handler 内读取的密钥字符串，用于已挂层时的校验。
- **敏感字段内存表示**：**`web_api_bearer_token`** 与 **`web_search_api_key`** 在 [`AgentConfig`](DEVELOPMENT.md) 内为 **secrecy `SecretString`**，**`Debug` / 结构化日志默认不打印明文**；源码中通过 **`ExposeSecret::expose_secret()`** 取用（`config` crate 再导出 **`ExposeSecret`**）。**`API_KEY`** 仍为仅环境变量，未并入 `AgentConfig`。

## 环境变量（`AGENT_*`）

以下为常用项；**完整键名与默认值以 `config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml` 为准**。**`API_KEY`** 仅环境变量，见下节「模型与 API」表格；热重载与密钥行为见上文「配置热重载」。

### 模型与 API

| 环境变量 | 说明 |
| --- | --- |
| `API_KEY` | 云厂商 / OpenAI 兼容后端的 Bearer token；`llm_http_auth_mode=bearer`（默认）时用于发往 `chat/completions` 等。**不写 TOML**。**`serve` / `repl` / `chat` 可在未设置时启动**；对话前须有关密钥：**Web** 的 **`client_llm.api_key`**、本环境变量、或 **REPL `/api-key`**。**`models` / `probe` 子命令**在 bearer 下仍要求此处非空。`llm_http_auth_mode=none`（如本地 Ollama）时可不设。 |
| `AGENT_API_BASE` | 覆盖 `api_base`。 |
| `AGENT_MODEL` | 覆盖 `model`。 |
| `AGENT_LLM_HTTP_AUTH_MODE` | `bearer`（默认，需 **`API_KEY`**）或 `none`（不向 `chat/completions` / `models` 发 `Authorization`，本地 Ollama 等可不设 **`API_KEY`**）。 |
| `AGENT_LLM_REASONING_SPLIT` | 覆盖 `llm_reasoning_split`。未在 TOML/环境变量设置时：**MiniMax 网关**（`model` 或 `api_base` 可识别为 MiniMax）**默认为开**（`true`），其它网关默认为关；见下文「MiniMax」。 |
| `AGENT_LLM_BIGMODEL_THINKING` | 为真时在请求体中带智谱 **`thinking: { "type": "enabled" }`**（GLM-5 深度思考；见下文「智谱 GLM」）。 |
| `AGENT_LLM_KIMI_THINKING_DISABLED` | 为真时在请求体中带 **`thinking: { "type": "disabled" }`**（关闭 Moonshot **kimi-k2.5** 默认思考；见下文「Moonshot（Kimi）」）。 |
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
| `AGENT_WEB_API_BEARER_TOKEN` | 受保护 Web API 的共享密钥；请求头 **`Authorization: Bearer …`** 或 **`X-API-Key: …`**（值相同，任选其一）。 |
| `AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK` | 非回环监听时是否允许无鉴权启动（高风险，仅可信环境）。 |

### 工作区与 Cursor 式规则

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_WORKSPACE_ALLOWED_ROOTS` | 逗号分隔，等价 `[agent] workspace_allowed_roots`。 |
| `AGENT_CURSOR_RULES_ENABLED` | 是否启用规则注入。 |
| `AGENT_CURSOR_RULES_DIR` | 规则目录（`*.mdc`）。 |
| `AGENT_CURSOR_RULES_INCLUDE_AGENTS_MD` | 是否并入 `AGENTS.md`。 |
| `AGENT_CURSOR_RULES_MAX_CHARS` | 注入长度上限。 |

**路径安全（与实现一致）**：`workspace_allowed_roots` 与每次请求对当前工作区根的重验可拒绝明显的 `..` 逃逸与**校验时刻**已指向根外的 symlink。在 **Unix** 上，**`read_file`**（`resolve_for_read_open`）与 **Web** 工作区列表、读/写/删文件等经 **`src/workspace_fs.rs`**：Linux 上使用 **`openat2` + `RESOLVE_IN_ROOT`** 在已打开的工作区根 fd 上解析相对路径并打开，将「策略校验后的路径字符串」与「实际 `open`」之间的竞态窗口收窄；工作区内 symlink 仍可跟随，但解析不得越过该根。**残余风险**：校验阶段仍依赖该时刻的 `canonicalize`；非 Linux、或未走 `workspace_fs` 的代码路径仍可能存在 TOCTOU；**`create_dir_all`** 等与按路径打开的组合亦未完全原子化。不可等同于内核沙箱；开放网络须配 **Web 鉴权**等。详见 **`src/path_workspace.rs`** 模块注释。

### 规划与分阶段规划

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_FINAL_PLAN_REQUIREMENT` | `never` / `workflow_reflection` / `always`。 |
| `AGENT_PLAN_REWRITE_MAX_ATTEMPTS` | 规划重写上限。 |
| `AGENT_PLANNER_EXECUTOR_MODE` | `single_agent` / `logical_dual_agent`。 |
| `AGENT_STAGED_PLAN_EXECUTION` | 是否启用分阶段规划。 |
| `AGENT_STAGED_PLAN_PHASE_INSTRUCTION` | 规划相说明/指令。 |
| `AGENT_STAGED_PLAN_ALLOW_NO_TASK` | 兼容旧变量，**已无效果**（`no_task` 约定见默认规划轮内嵌 schema）。 |
| `AGENT_STAGED_PLAN_FEEDBACK_MODE` | `fail_fast` / `patch_planner`。 |
| `AGENT_STAGED_PLAN_PATCH_MAX_ATTEMPTS` | `patch_planner` 补丁轮上限。 |
| `AGENT_STAGED_PLAN_ENSEMBLE_COUNT` | 逻辑多规划员份数（1–3，默认 1）。 |
| `AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM` | CLI / `chat` 无工具规划轮是否向 stdout 打印模型流（默认 `true`；见下文「分阶段规划」）。 |
| `AGENT_STAGED_PLAN_OPTIMIZER_ROUND` | 是否启用规划步骤优化轮（默认 `true`）。 |
| `AGENT_STAGED_PLAN_TWO_PHASE_NL_DISPLAY` | 为 `true` 时：无工具规划 JSON **定稿**后不向用户侧流式输出该 JSON，再追加一轮仅自然语言的补全请求（默认 `false`；见下文「分阶段规划」）。 |

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
| `AGENT_HEALTH_LLM_MODELS_PROBE` | 为 `1`/`true` 时 **`GET /health`** 附带对当前 **`api_base`** 的 **GET …/models** 连通性检查（列表接口，无 chat 计费）；默认关闭。 |
| `AGENT_HEALTH_LLM_MODELS_PROBE_CACHE_SECS` | 上述探测结果在进程内缓存秒数（**5–86400**，默认 **120**），减轻高频健康检查对上游的请求。 |
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
| `AGENT_CODEBASE_SEMANTIC_REBUILD_MAX_FILES` | **`rebuild_index`** 时最多**重新嵌入**的文件数（防超大仓；未改文件在增量模式下不计入）。 |
| `AGENT_CODEBASE_SEMANTIC_REBUILD_INCREMENTAL` | 整库 **`rebuild_index`** 是否默认**增量**（按 **`mtime`+`size`+内容 SHA256** 跳过未改文件）；**`false`** 则每次清空向量块与文件目录表后全量重嵌入。子目录 **`path`** 仍为子树全量替换。 |
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
| `AGENT_PROJECT_DEPENDENCY_BRIEF_INJECT_MAX_CHARS` | 由 `cargo metadata`（workspace resolve 边 + Mermaid）与**工作区根或 `frontend/` 子目录**（常见 npm 子项目路径，与 CrabMate 自带 `frontend-leptos` 无必然关系）的 `package.json` 依赖名节选组成；不含版本与 lockfile 全文；`0` 关闭该段。 |

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
| **`sub_agent_patch_write_extra_tools`** | 分阶段规划 **`executor_kind: patch_write`** 在默认补丁工具名之外**额外允许**的工具名（仍须在本会话已注册的工具列表中）。 |
| **`sub_agent_test_runner_extra_tools`** | 同上，针对 **`test_runner`** 角色。 |
| **`sub_agent_review_readonly_deny_tools`** | **`review_readonly`** 步内**显式禁止**的工具名（精确匹配；优先于只读判定）。 |

### 上下文与工具消息

| 环境变量 | 说明 |
| --- | --- |
| `AGENT_MAX_MESSAGE_HISTORY` | 保留消息条数上限。 |
| `AGENT_TOOL_MESSAGE_MAX_CHARS` | 单条 `role: tool` 发往模型前压缩阈值。 |
| `AGENT_TOOL_RESULT_ENVELOPE_V1` | `crabmate_tool` 信封 v1。 |
| `AGENT_TOOL_STATS_ENABLED` | 为 `true`/`1`/`yes`/`on` 时启用进程内工具调用统计，并在**新会话**首条 `system` 末尾附加短提示（见下）。 |
| `AGENT_TOOL_STATS_WINDOW_EVENTS` | 滑动窗口保留的调用事件条数（16–65536；与 TOML `agent_tool_stats_window_events` 一致）。 |
| `AGENT_TOOL_STATS_MIN_SAMPLES` | 某工具在窗口内总次数 ≥ 该值才参与提示（1–10000）。 |
| `AGENT_TOOL_STATS_MAX_CHARS` | 附录 Markdown 最大字符数（64–32768，超出截断）。 |
| `AGENT_TOOL_STATS_WARN_BELOW_SUCCESS_RATIO` | 成功率低于该阈值（0.0–1.0）且满足 `min_samples` 时提示；有失败时也会提示。 |
| `AGENT_MATERIALIZE_DEEPSEEK_DSML_TOOL_CALLS` | DeepSeek DSML 工具调用物化。 |
| `AGENT_THINKING_AVOID_ECHO_SYSTEM_PROMPT` | 是否在首条 `system` 末尾附思考纪律附录；默认等价 `true`。 |
| `AGENT_THINKING_AVOID_ECHO_APPENDIX` | 附录内联正文（非空则清除文件路径；若再设 `…_FILE` 则**文件优先**）。 |
| `AGENT_THINKING_AVOID_ECHO_APPENDIX_FILE` | 附录 Markdown 文件路径（与 `system_prompt_file` 相同解析规则）。 |
| `AGENT_CONTEXT_CHAR_BUDGET` | 上下文字符预算。 |
| `AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM` | 摘要后至少保留条数。 |
| `AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS` | 触发摘要的字符阈值。 |
| `AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES` | 摘要保留尾部消息数。 |
| `AGENT_CONTEXT_SUMMARY_MAX_TOKENS` | 摘要请求 max_tokens。 |
| `AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS` | 摘要转写最大字符。 |

**`[agent]` 对应 TOML 键（工具统计）**（可写入 `config.toml` / `.agent_demo.toml` 等）：`agent_tool_stats_enabled`、`agent_tool_stats_window_events`、`agent_tool_stats_min_samples`、`agent_tool_stats_max_chars`、`agent_tool_stats_warn_below_success_ratio`。统计为**单进程内存**、**全局**（不按 `conversation_id` 分桶）；**不**记录工具参数与完整输出。Web 侧**仅**在新建会话（无已存 `conversation_id` 种子）时拼入；CLI **`chat` / `repl`** 与 **`workspace_session::initial_workspace_messages`** 在「新起一轮首条 system」路径拼入，从磁盘恢复的会话仍以基底 system 对齐且不附加该段。

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

然后可不设环境变量 **`API_KEY`** 即启动 `serve` / `repl` / `chat`。若使用默认 **`llm_http_auth_mode=bearer`**（云端网关）而未 export **`API_KEY`**，进程仍可启动；须在 **Web「设置」**填写密钥或 **REPL `/api-key set …`** 后再对话，否则会返回 **`LLM_API_KEY_REQUIRED`** 等错误。**工具调用（function calling）**依赖模型与 Ollama 版本；若不稳定可先 **`--no-tools`** 验证对话。`crabmate config`（自检）**不要求** **`API_KEY`**。

## MiniMax（OpenAI 兼容）

MiniMax 提供 **`https://api.minimaxi.com/v1`**（与官方文档一致；亦可能见 **`https://api.minimax.io/v1`** 等别名，以控制台为准）下的 OpenAI 兼容 **`POST …/chat/completions`**。官方文档示例含 **`role: "system"`**（见 [OpenAI API 兼容](https://platform.minimaxi.com/docs/api-reference/text-openai-api)），但**线上接口仍常返回** HTTP 400 **`invalid message role: system`**。CrabMate 在识别为 **MiniMax**（**`model`** 形如 **`MiniMax-…`** / **`abab…`**，或 **`api_base`** 主机名含 **`minimax`**）时**自动**将系统提示**并入**相关 **`user`**，无需 TOML 配置。其它网关保留独立 **`system`** 条。

**本仓库已实测的 `model` 示例**（与 CrabMate OpenAI 兼容调用链联调）：**`MiniMax-M2.7`**、**`MiniMax-M2.7-highspeed`**、**`MiniMax-M2.5`**。更多模型名与能力以 MiniMax 控制台及官方 API 文档为准。

建议配置：

```toml
[agent]
api_base = "https://api.minimaxi.com/v1"
model = "MiniMax-M2.7"   # 或 M2.7-highspeed / M2.5 等；以控制台为准
llm_http_auth_mode = "bearer"
# llm_reasoning_split：可省略；未写时 MiniMax 网关默认为 true（思维链分离）
# llm_reasoning_split = false   # 若不需要 reasoning_split，可显式关闭
```

环境变量 **`API_KEY`** 填平台发放的密钥（与 DeepSeek 等一致，走 **`Authorization: Bearer`**）。**`llm_reasoning_split`** 为 **`true`**（含未写配置时 MiniMax 的默认值）时，请求体会包含 **`reasoning_split: true`**（与文档中 `extra_body={"reasoning_split": True}` 一致）；供应商若在流式 **`delta`** 中返回 **`reasoning_details`**（常见为带 **`text`** 的 JSON 数组），CrabMate 会将其**增量合并**进内部的 **`reasoning_content`** 流与终态消息，终端/Web 仍按现有「思考 / 正文」路径展示。非 MiniMax 网关未写该键时默认为 **`false`**；MiniMax 下不需要分离思维链时请显式 **`llm_reasoning_split = false`** 或 **`AGENT_LLM_REASONING_SPLIT=0`**。

### 思维链中减少复述系统提示

默认 **`thinking_avoid_echo_system_prompt = true`**（**`[agent]`** 键，嵌入默认见 **`config/default_config.toml`**，与 **`system_prompt_file`** 同节）：附录正文默认来自 **`thinking_avoid_echo_appendix_file`**（仓库内 **`config/prompts/thinking_avoid_echo_appendix.md`**，可**直接改该 Markdown** 无需重编）；亦可 **`thinking_avoid_echo_appendix`** 内联多行字符串。**优先级**：配置了非空 **`thinking_avoid_echo_appendix_file`** 时**读盘优先**于内联；均未配置时用编译嵌入的默认正文。经 **`tool_stats::augment_system_prompt`** 拼到新会话首条 **`system`** 末尾。仅为**软性约束**。关闭开关：**`thinking_avoid_echo_system_prompt = false`** 或 **`AGENT_THINKING_AVOID_ECHO_SYSTEM_PROMPT=0`**。仍复述时可再收紧自有 **`system_prompt`** 或换模型。

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

**严格节点覆盖（`final_plan_require_strict_workflow_node_coverage`，默认 `false`，环境变量 `AGENT_FINAL_PLAN_REQUIRE_STRICT_WORKFLOW_NODE_COVERAGE`）**：为 `true` 时，若**任一步**填写了 `workflow_node_id`，则 `steps` 中出现的 `workflow_node_id` 须覆盖该次工具结果中的**全部** `nodes[].id`（每 id 至少一步）。未填任何 `workflow_node_id` 时不额外强制。

**终答规划侧向 LLM（默认关闭）**：**`final_plan_semantic_check_enabled`**（`AGENT_FINAL_PLAN_SEMANTIC_CHECK_ENABLED`，默认 `false`）在 **`final_plan_requirement = workflow_reflection`** 且本轮已要求终答含规划时，静态规则通过后若可自历史构造工具摘要，则再发**一次**无工具短请求，请模型判断规划与最近工具结果是否明显矛盾。侧向模型**优先**只输出 JSON：`{"consistent":true}` 或 `{"consistent":false,"violation_codes":["…"],"rationale":"…"}`；仍**兼容**旧式单行 `CONSISTENT` / `INCONSISTENT`。判定不一致时追加重写 user：正文含 **`crabmate_plan_semantic_feedback` v1** 的 `json` 围栏（`violation_codes`、`rationale` 可选），再接规划 JSON 重写说明（计入 `plan_rewrite_max_attempts`）。**`final_plan_semantic_check_max_non_readonly_tools`**（`AGENT_FINAL_PLAN_SEMANTIC_CHECK_MAX_NON_READONLY_TOOLS`，默认 `0`，合法 0–32）：摘要中额外收录的非只读工具条数上限；`0` 时仍收录内置高风险名（如 `run_command`、`workflow_execute` 等）与只读工具。**`final_plan_semantic_check_max_tokens`**（`AGENT_FINAL_PLAN_SEMANTIC_CHECK_MAX_TOKENS`，默认 `256`，合法 32–1024）：侧向请求的 `max_tokens`。API 失败或无法解析模型答复时**视为一致**（fail-open），避免阻断主循环。

## 规划重写（`plan_rewrite_max_attempts`）

规划不合格时追加「请重写」的上限；超过后流式前端可能收到 `code: plan_rewrite_exhausted`（可选同级 `reason_code` 子码，见 `docs/SSE_PROTOCOL.md`）。

## 逻辑双 agent（`planner_executor_mode = logical_dual_agent`）

先无工具规划轮，再执行器循环；planner 上下文会过滤 `role: tool` 正文。与 `staged_plan_execution` 并存时本模式优先。

## 分阶段规划（`staged_plan_execution`）

在 `planner_executor_mode = single_agent` 且开启时，每条用户消息先走无工具规划轮，再按 `steps` 执行。`no_task` + 空 `steps` 可跳过执行。规划 JSON 无法解析时降级为常规工具循环。API 调用通常多于关闭时。

**步级子代理（规划 JSON 可选字段）**：在 `agent_reply_plan` v1 的每个 `steps[]` 中可写 **`executor_kind`**：`review_readonly`（仅语义只读工具）、`patch_write`（只读 + 受限补丁写）、`test_runner`（只读 + 内置测试运行器如 `cargo_test` / `pytest_run` 等，不含任意 `run_command`）。该步内外层循环的 **OpenAI tools 列表**会相应收窄，越权调用在工具层被拒绝并写入对话（拒绝正文会附带本步允许的工具名摘要）；省略该字段则与本功能推出前行为一致。只读/写判定与 **`[tool_registry] write_effect_tools`** 一致；补丁类与测试类在默认名单外可通过 **`sub_agent_patch_write_extra_tools`** / **`sub_agent_test_runner_extra_tools`** 扩充；**不**改变 `run_command` 白名单或 MCP 审批。SSE **`staged_plan_step_started`** / **`staged_plan_step_finished`** 负载均可选带 **`executor_kind`** 字符串（与 JSON 中蛇形值一致），便于前端展示当前子代理角色。**`patch_planner`** 合并补丁规划时，若补丁步省略 `executor_kind`，服务端会按**同一下标**从原规划继承（并打 `debug` 日志），减少子代理边界在补丁后静默丢失。

**步级反馈（`staged_plan_feedback_mode`）**：默认 `fail_fast`（某步子循环 `Err` 或步内存在失败工具结果时，整轮计划按失败结束）。设为 `patch_planner` 时，会向规划器注入简短反馈并无工具重跑规划轮，将补丁 `steps` 与「当前步及之后」合并后继续执行（受 `staged_plan_patch_max_attempts` 限制，多耗 API）。

**CLI 规划轮终端输出（`staged_plan_cli_show_planner_stream`，默认 `true`，环境变量 `AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM`）**：仅影响 **CLI / `chat` 等 `out: None` 路径** 下，**无工具规划轮**与 **`patch_planner` 补丁规划轮**是否向 stdout 流式或整段打印模型原文（`Agent:` 前缀及正文）。设为 `false` 时这些轮次不在终端打印模型输出，仍保留 `staged_plan_notice` 队列摘要、分步注入 user 转录与后续执行步的助手输出；Web SSE 路径不受影响。

**规划步骤优化轮（`staged_plan_optimizer_round`，默认 `true`，环境变量 `AGENT_STAGED_PLAN_OPTIMIZER_ROUND`）**：在首轮 `agent_reply_plan` v1 解析成功且 `steps` 不少于 2 时，再追加一轮无工具请求，请模型合并**无数据依赖**的只读探查步，并提示在同一执行步内对「可同轮并行批处理」的内建工具（与执行层 `parallel_readonly_tools` 判定一致，不限于 `read_file`）发起多次调用。解析失败或用户取消优化轮时沿用首轮规划；成功则追加优化轮 assistant 并采用新 `steps`（多一次 API）。

**优化轮门控（`staged_plan_optimizer_requires_parallel_tools`，默认 `true`，环境变量 `AGENT_STAGED_PLAN_OPTIMIZER_REQUIRES_PARALLEL_TOOLS`）**：为 `true` 时，仅当本会话 `tools_defs` 中经服务端判定**至少有一个**可同轮并行批处理的内建工具名时，才在 `steps.len() >= 2` 且开启优化轮时发起上述优化请求；若 CSV 为空则跳过该轮以省 API（优化提示主要围绕并行工具列表）。设为 `false` 可恢复旧行为：只要步数与 `staged_plan_optimizer_round` 满足即始终调用优化轮。

**逻辑多规划员与合并（`staged_plan_ensemble_count`，默认 `1`，环境变量 `AGENT_STAGED_PLAN_ENSEMBLE_COUNT`，合法值钳制为 1–3）**：`1` 表示关闭。为 `2` 或 `3` 时，在首轮规划写入历史后，再**串行**发起 1 或 2 次无工具「独立规划员」请求（通过服务端注入的 user 正文区分角色；**辅助规划员的 assistant 不写入会话历史**，仅合并轮的 user+assistant 会保留），最后追加一轮「合并多份草案」的无工具请求，产出单一 `steps` 后再进入上述步骤优化轮（若启用）。仍为**同一进程、同一模型与密钥**；不保证质量更优，且 **API 次数与费用明显增加**（例如 `3` + 优化轮 ≈ 首轮外再多 3 次规划类调用）。某辅助轮解析失败时停止追加规划员；若最终有效草案不足 2 份则不跑合并轮。

**Ensemble 门控（`staged_plan_skip_ensemble_on_casual_prompt`，默认 `true`，环境变量 `AGENT_STAGED_PLAN_SKIP_ENSEMBLE_ON_CASUAL_PROMPT`）**：在 `staged_plan_ensemble_count > 1` 时，若从消息历史回溯到的**本轮用户正文**经简单启发式判定为寒暄或极短输入，则跳过逻辑多规划员与合并轮，直接沿用首轮规划（省多次规划 API）。设为 `false` 则始终按 `staged_plan_ensemble_count` 跑满（在解析成功的前提下）。

**两轮展示（`staged_plan_two_phase_nl_display`，默认 `false`，环境变量 `AGENT_STAGED_PLAN_TWO_PHASE_NL_DISPLAY`）**：为 `true` 时，在 `agent_reply_plan` v1 **已解析并入史**之后（含可选的逻辑多规划员与合并轮、步骤优化轮；`no_task` 路径在转入常规循环前亦同），对**上述无工具规划类轮次**在调用 `complete_chat_retrying` 时**不向用户侧流式输出**规划 JSON（`out: None` 且抑制 `render_to_terminal`，与 `staged_plan_cli_show_planner_stream` 相与后再决定是否打印终端）。随后追加一条桥接 **user**（`staged_sse::staged_plan_nl_followup_user_body`：口语续问 + 与分步注入同类的展示层隐藏前缀，聊天区**不**展示该条，避免被模型叙述成「用户下发了系统指令」），再发起一轮**无工具**补全：模型应答作为**用户可见**自然语言流式/整段下发；会话历史中保留 JSON 助手条 + 桥接 user + NL 助手条。**未**使用供应商 `response_format: json_object` 等 API 级强约束，首轮 JSON 仍依赖围栏/正文解析。**`patch_planner`** 在步内成功后再次产出的规划 JSON **不**自动触发上述 NL 补全轮（与首轮定稿路径区分）。

## SyncDefault 工具 Docker 沙盒（`sync_default_tool_sandbox_mode`）

### 模式与覆盖范围

- **`none`（默认）**：与历史一致，在 Agent 进程内 `spawn_blocking` 执行 `HandlerId::SyncDefault` 工具；**`run_command` 等**也在宿主执行。
- **`docker`**：**SyncDefault** 以及 **`run_command` / `run_executable` / `get_weather` / `web_search` / `http_fetch` / `http_request`** 在宿主完成白名单与审批（若有）后，每次调用经 **[bollard](https://docs.rs/bollard)** 走 **Docker Engine HTTP API** 创建并运行一次性容器（等价于 `docker run --rm -i`）：挂载当前工作区到容器内 **`/workspace`**（读写），只读挂载**当前正在运行的宿主 `crabmate` 可执行文件**到 **`/crabmate`**，在容器内执行 **`crabmate tool-runner-internal`**（由服务端生成临时 JSON 配置并只读挂入容器）。**Linux/macOS** 默认连接本地 Unix 套接字（与 `docker` CLI 相同）；**`DOCKER_HOST`** 在部分环境下亦可由 bollard 解析。
- **不进入沙盒**：**`workflow_execute`**、**MCP 代理工具**（`mcp__*`）仍只在宿主执行。

**bollard 编译特性（维护者）**：根目录 **`Cargo.toml`** 对 **bollard** 使用 **`default-features = false`**，仅启用 **`http`**、**`pipe`**（本地 **`unix://`**、Windows named pipe、明文 **`tcp://`** / **`http://`** 的 **`DOCKER_HOST`**，可减小依赖与二进制体积）。若 **`DOCKER_HOST` 为 `https://`** 或设置 **`DOCKER_TLS_VERIFY`**，须在 **`bollard`** 的 **`features`** 中**追加 `ssl`** 并重新编译（会链接 **rustls** 等）。

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

仓库内默认正文含工具与任务拆分等约定（例如**同一工作区路径在未被修改前不要重复 `read_file`**），并约定用户以中文为主时助手**解释性文字用简体中文、减少中英夹杂**（代码与专有名词等除外）。完全自定义时可改 `config/prompts/default_system_prompt.md` 或换用自有路径。

## 多角色（agent_roles）

在全局 `system_prompt` 之外，可为**命名 id** 配置不同的首条 `system` 正文（每条在加载时**同样**经 `cursor_rules_*` 合并，与全局一致）。

- **定义方式（二选一或混用，后加载覆盖同 id 字段）**  
  1. 主配置文件中的 **`[[agent_roles]]`** 表数组：每行含 **`id`**，以及 **`system_prompt`** 和/或 **`system_prompt_file`**（其一即可；`system_prompt` 为空字符串表示**沿用**全局合并后的 `system_prompt`）。  
  2. 仓库根 **`config/agent_roles.toml`**（未使用 **`--config`** 时）；若使用 **`crabmate --config path/to/foo.toml`**，则读取 **`path/to/agent_roles.toml`**（与主配置**同目录**）。文件形态为 **`[agent_roles]`** + **`default_role`** + **`[agent_roles.roles.<id>]`** 子表（见 `config/agent_roles.toml` 注释示例）。
- **默认角色**：`[agent]` 中 **`default_agent_role`**，或 `agent_roles.toml` 的 **`[agent_roles] default_role`**，或环境变量 **`AGENT_DEFAULT_AGENT_ROLE`**。须指向已定义的角色 id；未配置默认时，未显式选角则使用全局 **`system_prompt`**。
- **可选 `allowed_tools`（多角色工作台）**：在 **`[[agent_roles]]`** 行或 **`[agent_roles.roles.<id>]`** 下可写字符串数组 **`allowed_tools`**。非空时：本角色**仅允许**列表中的内置工具名；列表中含字面量 **`mcp`** 时允许所有 **`mcp__*`** MCP 代理工具。省略或空数组表示**不限制**（与历史行为一致）。工具白名单按 **请求 `agent_role` → 会话持久化 `active_agent_role` → `default_agent_role_id`** 解析命名 id，与首条 `system` 所用角色对齐。
- **Web**：`POST /chat`、`POST /chat/stream` 可选 JSON 字段 **`agent_role`**（与 `conversation_id` 同类字符集，最长 64）。**新会话**（服务端尚无该 `conversation_id` 历史）：与历史相同，用于首轮 `system`。**已有会话**：若与 SQLite/内存中持久化的 **`active_agent_role`** 不同，则**仅刷新首条 `system`** 并更新持久化角色，**保留**后续对话；省略 `agent_role` 则沿用上次持久化角色。启用 **`allowed_tools`** 时，每轮按上条规则裁剪送进模型的工具列表并在执行层拒绝越权调用。
- **CLI**：全局 **`--agent-role <id>`**（`repl` / `chat` 等）。与 **`--system-prompt-file`** 互斥。`chat` 在**未**使用 **`--messages-json-file`** 时，该 id 用于构造首条 system（含 **`--message-file`** 首轮）；**`allowed_tools`** 与 Web 同源，按该 id（及配置默认角色）裁剪工具。
- **REPL**：**`/agent list`** 先列出内建伪 id **`default`**（未显式选用命名角色；语义同 Web 未传 **`agent_role`**：有 **`default_agent_role_id`** 则用其条目，否则用全局 **`system_prompt`**），再列配置中的命名 id（与 **`GET /status`** 的 **`agent_role_ids`** 同源）；**`/agent set <id>`** / **`/agent set default`**：校验 id 后更新当前选用角色，并**仅替换首条 `system`**（**不清空**后续消息），便于在同一会话内切换「实现 / 审查」等人格；**`default`** 清除显式命名角色。
- **热重载**：**`POST /config/reload`** / **`/config reload`** 会重载角色表（与 `system_prompt` 一致）。
- **`GET /status`**：返回 **`agent_role_ids`**（升序）、**`default_agent_role_id`**，供前端展示角色下拉等。

## Cursor-like 规则注入

`cursor_rules_enabled` 为真时读取 `cursor_rules_dir` 下 `*.mdc`（可附加 `AGENTS.md`），拼到系统提示词末尾，长度受 `cursor_rules_max_chars` 限制。

## 上下文窗口

请求前会压缩 `messages`：条数上限、`context_char_budget`、可选 LLM 摘要等。其中 **`tool_message_max_chars`**（`AGENT_TOOL_MESSAGE_MAX_CHARS`）：单条 `role: tool` 在**发往模型前**若超长则压缩；启用 **`tool_result_envelope_v1`** 时对 `crabmate_tool.output` 采用**首尾采样**并附带 `output_truncated` 等字段（见 **`docs/DEVELOPMENT.md`**）。详见 `config/tools.toml`。

## Web 对话队列（`chat_queue_*`）

`/chat` 与 `/chat/stream` 经有界队列调度；满时 **503**、`QUEUE_FULL`。`/status` 返回队列与 `per_active_jobs` 等字段。

- **可选 `client_llm` 对象**（仅作用于**本次入队**的对话任务，**不写**服务端配置）：可含 **`api_base`**、**`model`**、**`api_key`**（均可选；trim 后为空表示该字段不覆盖）。若携带非空 **`api_key`**，该任务对 LLM 的 HTTP 请求按 **Bearer** 使用该密钥（即使进程配置为 **`llm_http_auth_mode = none`**）。长度上限：`api_base` 2048 字符、`model` 512、`api_key` 16384；不合法时 **400**、错误码 **`INVALID_CLIENT_LLM`**。**安全提示**：密钥会出现在发往 CrabMate 后端的 **JSON 请求体** 中，请仅在 **HTTPS** 等可信链路使用；Web「设置」可将密钥存入 **`localStorage`**（密码框不回显已存值），公共电脑勿用。
- **可选 `client_sse_protocol`**（`u8`）：客户端声明其实现的 SSE 控制面版本，与 workspace crate **`crabmate-sse-protocol`** 的 **`SSE_PROTOCOL_VERSION`** 对齐。官方 Web 随 **`POST /chat`** / **`POST /chat/stream`** 发送。**大于**服务端版本时 **400**（**`SSE_CLIENT_TOO_NEW`**）；**`0`** 为 **400**（**`INVALID_SSE_CLIENT_PROTOCOL`**）。省略则不校验。首帧 **`sse_capabilities.supported_sse_v`** 与协商语义见 **`docs/SSE_PROTOCOL.md`**。

## 只读工具并行（`parallel_readonly_tools_max`）

限制同轮多只读工具进入 blocking 池的并发数： eligible 批含内建只读 **`SyncDefault`**、**`http_fetch`**（GET/HEAD）、**`get_weather`**、**`web_search`**（不含 **`http_request`**、**`run_command`**、MCP 等）。构建锁类（如 **`cargo_*`**、**`npm_*`**）整批降级为串行。

## HTTP 客户端

进程内共享 `reqwest::Client`（连接池、Keep-Alive）。细节见 **`docs/DEVELOPMENT.md`** 中 `http_client` 说明。

## 常用模型 ID

- `deepseek-chat`（默认）
- `deepseek-reasoner`（推理链更长）
