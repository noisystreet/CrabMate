# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate logo" width="220" />
</p>

CrabMate 是一个基于 **DeepSeek API** 从零实现的简易 Rust AI Agent，支持**工具调用**（Function Calling），能在工作区内执行命令、查看/编辑文件并给出自然语言回复。

## 功能概览

- **DeepSeek 对话**：多模型、流式回复、工具调用（Function Calling）；工作区内读文件、跑命令、改代码等。
- **可选 [MCP](https://modelcontextprotocol.io/)（stdio）**：配置 `mcp_enabled` 与 `mcp_command` 后合并远端工具（`mcp__{slug}__{name}`）；详见下方配置与环境变量 `AGENT_MCP_*`。
- **Web / CLI**：`serve` 提供 Web UI 与 API；默认 `repl`；`chat` 适合脚本与 CI。含工作区面板、可选任务侧栏（`/tasks`，进程内存、重启清空）、会话可选 SQLite、流式审批等，详见「基本使用」。
- **工具清单、JSON 参数示例与排障**：见 **[`docs/TOOL_EXAMPLES.md`](docs/TOOL_EXAMPLES.md)**；架构与协议见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)、[`docs/SSE_PROTOCOL.md`](docs/SSE_PROTOCOL.md)。

## 文档与维护

- **架构与二次开发**：见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)（模块职责等）；**`/chat/stream` SSE 控制面**（版本 `v`、`error`/`code`、`tool_result` 等）见 [`docs/SSE_PROTOCOL.md`](docs/SSE_PROTOCOL.md)，与 `frontend/src/api.ts` 对齐。
- **待办清单**：[`docs/TODOLIST.md`](docs/TODOLIST.md) 仅列未完成项；**上半**为全局优先级（P0–P5），**下半**为按模块的中长期方向；**完成某项后应从该文件删除对应条目**（不要只打勾保留），约定详见 `DEVELOPMENT.md`。
- **新功能**：合并用户可见能力时，请同步更新本 README（功能/命令/配置）与/或 `DEVELOPMENT.md`（架构与协议）；**新增或变更工具参数示例**时同步 [`docs/TOOL_EXAMPLES.md`](docs/TOOL_EXAMPLES.md)。

## 部署与安全提示

- **默认仅本机监听**（`--serve`）：绑定 **`127.0.0.1`**，局域网其它设备默认无法直连。若需局域网访问，请显式使用 `--host 0.0.0.0` 或设置环境变量 `AGENT_HTTP_HOST=0.0.0.0`（未传 `--host` 时生效）。当监听**非 loopback** 地址时，若未配置 `web_api_bearer_token` / `AGENT_WEB_API_BEARER_TOKEN`，服务默认拒绝启动；如确需无鉴权运行，需显式设置 `allow_insecure_no_auth_for_non_loopback=true`（或 `AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`，不安全）。
- **Web Bearer 鉴权（可选）**：设置 `web_api_bearer_token` 后，`/chat`、`/workspace`、`/tasks`、`/upload` 等接口要求 `Authorization: Bearer <token>`。内置前端会从浏览器 `localStorage["crabmate-api-bearer-token"]` 读取 token 并自动附带该请求头（可在浏览器控制台手动设置）。
- **工作区**：Web 端通过 `POST /workspace` 设置的路径**必须已存在且为目录**，且（`canonicalize` 后）须落在配置的**允许根目录**之下，并避开敏感系统目录黑名单（如 `/proc`、`/sys`、`/dev`、`/etc`、`/usr`）。未配置 `workspace_allowed_roots` / `AGENT_WORKSPACE_ALLOWED_ROOTS` 时，仅允许 **`run_command_working_dir` 及其子目录**；若配置了多个根路径，则 `run_command_working_dir` 本身也须落在其中某一根之下（否则启动报错）。**每次**访问 `GET/POST /workspace`、`/workspace/file`、`/workspace/search` 等会再次解析当前工作区根并校验其仍在允许根内（防止配置或磁盘变化后会话仍指向越界路径）。子路径与文件工具共用 `path_workspace`：`..` 规范化后不得越界；存在路径再 `canonicalize` 以跟随 symlink；写入路径另校验「最近存在祖先」的 canonical 目标仍在根内。若你未配置 `web_api_bearer_token`，请勿在不可信网络暴露本服务。
- **联网搜索 Key**：`web_search_api_key` 与 DeepSeek 的 `API_KEY` 无关；若写入配置文件，请妥善保管文件权限，避免泄露第三方搜索配额。
- **建议**：公网或不可信网络请配合反向代理、鉴权、TLS、防火墙等自行加固。
- **会话 SQLite**（`conversation_store_sqlite_path`）：库文件落在服务端本地路径；多用户/共享主机时请限制文件权限，避免将库放在他人可写目录。

## 环境

- Rust 1.70+
- 环境变量：`API_KEY`，值为 [DeepSeek 开放平台](https://platform.deepseek.com/) 的 API Key

## 配置与多模型切换

**默认配置**来自项目根目录的 `default_config.toml`（含 `api_base`、`model`）。可在当前工作目录用 `config.toml` 或 `.agent_demo.toml` 覆盖，再被环境变量覆盖（为了兼容早期命名，保留 `.agent_demo.toml` 作为别名）。

1. **环境变量**（优先级最高）  
   - `AGENT_API_BASE`：API 基础 URL  
   - `AGENT_MODEL`：模型 ID  
   - `AGENT_SYSTEM_PROMPT`：系统提示词（内联）  
   - `AGENT_SYSTEM_PROMPT_FILE`：系统提示词文件路径（与上二选一，文件优先）  
   - `AGENT_CURSOR_RULES_ENABLED`：是否启用 Cursor-like 规则注入（`1/true/yes/on` 启用；`0/false/no/off` 关闭）  
   - `AGENT_CURSOR_RULES_DIR`：规则目录（默认 `.cursor/rules`，读取其中 `*.mdc`）  
   - `AGENT_CURSOR_RULES_INCLUDE_AGENTS_MD`：启用规则注入时是否额外附加工作区根 `AGENTS.md`（默认 `true`）  
   - `AGENT_CURSOR_RULES_MAX_CHARS`：规则附加段最大字符数（默认 `48000`，超出截断）  
   - `AGENT_FINAL_PLAN_REQUIREMENT`：终答是否必须含结构化 `agent_reply_plan`，取值 `never` / `workflow_reflection` / `always`（与 `[agent] final_plan_requirement` 一致，默认 `workflow_reflection`）  
   - `AGENT_PLAN_REWRITE_MAX_ATTEMPTS`：规划不合格时最多重写轮次（默认 `2`，与 `[agent] plan_rewrite_max_attempts` 一致；用尽后 SSE 带 `code=plan_rewrite_exhausted`）  
   - `AGENT_HTTP_HOST`：Web 监听 IP（如 `0.0.0.0`）；**未**传 `--host` 时生效，默认仍为 `127.0.0.1`  
   - `AGENT_TEMPERATURE`：采样温度 0～2（与 `[agent] temperature` 一致）  
   - `AGENT_LLM_SEED`：可选整数，写入每次 `chat/completions` 请求的 **`seed`** 字段（与 `[agent] llm_seed` 一致；未设置则请求体不带 `seed`）  
   - `AGENT_CHAT_QUEUE_MAX_CONCURRENT`、`AGENT_CHAT_QUEUE_MAX_PENDING`：`/chat` 与 `/chat/stream` 的进程内任务并发与排队上限（超出排队返回 HTTP 503，`code=QUEUE_FULL`）
   - `AGENT_PARALLEL_READONLY_TOOLS_MAX`：单轮内**多只读工具并行**时 `spawn_blocking` 的并发上限（默认与 `chat_queue_max_concurrent` 相同；与 `[agent] parallel_readonly_tools_max` 一致）
   - `AGENT_ALLOWED_COMMANDS`：`run_command` 白名单，**逗号分隔**命令名（覆盖 `[agent] allowed_commands`；默认见 `default_config.toml`，含常用 coreutils、`grep`/`diff`、`git`/`cargo`（dev）、`jq`、`zcat` 等；**prod** 用 `allowed_commands_prod` 更窄）
   - **MCP（stdio）**：`AGENT_MCP_ENABLED`（`1`/`true`/`yes`/`on`）；`AGENT_MCP_COMMAND`（整行命令，空格分词）；`AGENT_MCP_TOOL_TIMEOUT_SECS`（`tools/call` 超时秒数，默认与 `command_timeout_secs` 一致）
   - `AGENT_CONVERSATION_STORE_SQLITE_PATH`：Web 会话 SQLite 文件路径（非空则持久化；与 `[agent] conversation_store_sqlite_path` 一致）
   - `AGENT_MEMORY_FILE_ENABLED`、`AGENT_MEMORY_FILE`、`AGENT_MEMORY_FILE_MAX_CHARS`：Web 首轮工作区备忘注入（与 `[agent]` 同名项一致）
   - **长期记忆（默认关闭）**：`AGENT_LONG_TERM_MEMORY_ENABLED`；`AGENT_LONG_TERM_MEMORY_SCOPE_MODE`（当前仅 `conversation`）；`AGENT_LONG_TERM_MEMORY_VECTOR_BACKEND`：`disabled`（仅按时间取最近片段）或 **`fastembed`**（本地 CPU 嵌入 + 余弦相似度；首次运行可能下载 ONNX 模型）；`qdrant` / `pgvector` 配置项保留但启动会报错（尚未接入）。另有 `AGENT_LONG_TERM_MEMORY_STORE_SQLITE_PATH`、`AGENT_LONG_TERM_MEMORY_TOP_K`、`AGENT_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK`、`AGENT_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX`、`AGENT_LONG_TERM_MEMORY_ASYNC_INDEX`、`AGENT_LONG_TERM_MEMORY_MAX_ENTRIES`、`AGENT_LONG_TERM_MEMORY_INJECT_MAX_CHARS`。Web：**已配置 `conversation_store_sqlite_path` 时会话库与长期记忆共用同一 SQLite**；若会话仅内存模式，须显式设置 `long_term_memory_store_sqlite_path` 否则不持久化记忆。CLI：默认 `run_command_working_dir/.crabmate/long_term_memory.db`。`GET /status` 返回 `long_term_memory_*` 便于确认是否就绪。多用户无 Bearer 鉴权时，勿依赖 `conversation_id` 作为安全边界。
  - `AGENT_PLANNER_EXECUTOR_MODE`：规划器/执行器模式，`single_agent`（默认，历史行为）或 `logical_dual_agent`（阶段 1：同进程逻辑双 agent，规划轮只看用户/助手自然语言，不看 `tool` 正文）
  - `AGENT_STAGED_PLAN_EXECUTION`：设为 `1`/`true`/`yes`/`on` 启用分阶段规划（仅在 `planner_executor_mode=single_agent` 下生效）；其它或未设置为关闭（与 `[agent] staged_plan_execution` 一致）
   - `AGENT_STAGED_PLAN_PHASE_INSTRUCTION`：规划轮追加的 **system** 文案；空或未设置则用内置默认（与 `[agent] staged_plan_phase_instruction` 一致）
   - `AGENT_STAGED_PLAN_ALLOW_NO_TASK`：内置规划说明是否包含「无具体任务则 `no_task` + 空 `steps`」；`1`/`true`/`yes`/`on` 为开启（默认与 `[agent] staged_plan_allow_no_task` 一致）
   - **联网搜索**（`web_search` 工具）：`AGENT_WEB_SEARCH_PROVIDER`（`brave` / `tavily`）、`AGENT_WEB_SEARCH_API_KEY`、`AGENT_WEB_SEARCH_TIMEOUT_SECS`、`AGENT_WEB_SEARCH_MAX_RESULTS`（1～20，默认 8）
   - **`http_fetch`**：`AGENT_HTTP_FETCH_ALLOWED_PREFIXES`（逗号分隔 URL 前缀）、`AGENT_HTTP_FETCH_TIMEOUT_SECS`、`AGENT_HTTP_FETCH_MAX_RESPONSE_BYTES`（与 `default_config.toml` / `[agent]` 中同名项对应）
   - **上下文窗口**（长会话防爆 token，见 `default_config.toml`）：`AGENT_MAX_MESSAGE_HISTORY`、`AGENT_TOOL_MESSAGE_MAX_CHARS`、**`AGENT_TOOL_RESULT_ENVELOPE_V1`**（默认 `true`：`role: tool` 写入 `crabmate_tool` JSON 信封，含 `summary`/`ok`/`output` 等，便于聚合；`false` 恢复纯原文）、**`AGENT_MATERIALIZE_DEEPSEEK_DSML_TOOL_CALLS`**（默认 `true`：API 无可用原生 `tool_calls` 时从正文 DSML 物化；`false` 则**仅信任 API `tool_calls`**）、`AGENT_CONTEXT_CHAR_BUDGET`、`AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM`、`AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS`（`0` 关闭 LLM 摘要）、`AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES`、`AGENT_CONTEXT_SUMMARY_MAX_TOKENS`、`AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS`
   - **终端会话文件（CLI REPL）**：`AGENT_TUI_LOAD_SESSION_ON_START` / `[agent] tui_load_session_on_start` 为 `true` 时，REPL 启动从 `.crabmate/tui_session.json` 恢复历史；默认 `false`（仅空白会话 + 当前 `system_prompt`）。**若启用加载**：`AGENT_TUI_SESSION_MAX_MESSAGES` / `[agent] tui_session_max_messages` 限制总消息条数（含 `system`），超出则丢弃最旧非 system 消息（默认 `400`，有效范围 `2`～`50000`）
   - **Web 工作区白名单**：`AGENT_WORKSPACE_ALLOWED_ROOTS`（逗号分隔绝对或相对路径，相对路径相对**进程启动时当前目录**）；与 `[agent] workspace_allowed_roots` 数组等价。省略或空列表表示仅允许 `run_command_working_dir` 下路径；`GET /status` 返回 `workspace_allowed_roots_count` 便于确认策略宽度。
   ```bash
   export AGENT_MODEL=deepseek-reasoner
   cargo run
   ```
2. **配置文件**：`config.toml` 或 `.agent_demo.toml`（可只写要覆盖的项）：
   ```toml
   [agent]
   api_base = "https://api.deepseek.com/v1"
   model = "deepseek-reasoner"
   # 系统提示词：内联或从文件加载
   # system_prompt = "你是专业的助手。"
   # system_prompt_file = "system_prompt.txt"
   # Cursor-like 规则注入（可选）
   # cursor_rules_enabled = true
   # cursor_rules_dir = ".cursor/rules"
   # cursor_rules_include_agents_md = true
   # cursor_rules_max_chars = 48000
   ```
   可参考 `config.toml.example`。

**终答规划策略**（`[agent] final_plan_requirement`）：控制模型以**非 tool_calls**结束一轮时，是否必须嵌入可解析的 `agent_reply_plan` JSON（见 `docs/DEVELOPMENT.md`）。`workflow_reflection` 为默认：仅在工作流反思首轮注入「下一步须带规划」指令后启用校验；`never` 关闭该校验；**`always`（实验性）** 对**每一次**终答都校验：只要模型以正文结束（非 `tool_calls`）且规划 JSON 不合格，就会占用 `plan_rewrite_max_attempts` 额度并可能追加多轮 `chat/completions`，**API 费用与延迟明显高于**默认策略，适合强合规/审计场景或调试规划格式；日常对话、低成本场景请保持 `workflow_reflection` 或 `never`。若近期存在 `workflow_validate_only` 结果，服务端还会按 `spec.layer_count` 要求规划步骤条数不少于层数。

**规划重写次数**（`[agent] plan_rewrite_max_attempts`）：不合格时追加「请重写」user 消息的上限；超过后结束本轮，流式场景下前端会收到 `error` + `code: plan_rewrite_exhausted`。

**阶段 1：逻辑双 agent**（`[agent] planner_executor_mode` / `AGENT_PLANNER_EXECUTOR_MODE`）：设为 `logical_dual_agent` 时，每条用户消息先进入规划轮（planner，仅无工具），解析 `agent_reply_plan` 后逐步注入执行器（executor）外层循环。与 `single_agent` 差异：planner 上下文会过滤 `role: tool` 正文，仅保留用户/助手自然语言，降低工具噪声对规划的干扰；executor 仍按现有工具与审批策略执行。该模式与 `staged_plan_execution` 目标类似，但优先级更高（启用后直接走逻辑双 agent 路径）。

**分阶段规划（单 agent 模式）**（`[agent] staged_plan_execution` / `AGENT_STAGED_PLAN_EXECUTION`）：在 `planner_executor_mode=single_agent` 且本项为 `true` 时，**每条用户消息**先走一轮**无工具** API 调用，要求模型产出可解析 `agent_reply_plan` v1；解析成功后按 `steps` 顺序逐步执行。若模型判定用户**未提出具体可执行任务**（寒暄、致谢等），应在 JSON 中设 **`"no_task": true` 且 `"steps": []`**，服务端**跳过后续分步注入**，直接转入常规对话/工具循环（与关闭分阶段规划时一致）。**`staged_plan_allow_no_task`**（默认 `true`，环境变量 **`AGENT_STAGED_PLAN_ALLOW_NO_TASK`**）：为 `true` 时内置规划说明会包含上述约定；为 `false` 时从内置说明中省略（自定义 `staged_plan_phase_instruction` 不受影响）。若规划 JSON 无法解析，**不会**中断整轮对话：规划轮助手正文会写入消息列表并**自动转入**常规工具循环。若模型在规划轮用正文里的 **DeepSeek DSML**（如 `<｜DSML｜invoke>`）描述工具调用而无 `agent_reply_plan` JSON，服务端会先**丢弃**该轮 API 可能附带的原生 `tool_calls`（避免与 `tool_choice: none` 冲突、阻塞 DSML 物化），再**物化** DSML 为 `tool_calls` 并视情况**执行**，然后进入常规循环（与关闭分阶段时一致）。**API 调用次数与费用通常明显高于关闭时**。

**系统提示词**：在 `default_config.toml` 中通过 `system_prompt`（多行字符串）或 `system_prompt_file`（文件路径）配置；若同时设置，以文件内容为准。未配置则启动报错。

**Cursor-like 规则注入**：当 `cursor_rules_enabled=true`（或 `AGENT_CURSOR_RULES_ENABLED=1`）时，服务启动会自动读取 `cursor_rules_dir` 下全部 `*.mdc`（按文件名排序），并按配置可选附加工作区根 `AGENTS.md`，统一拼接到系统提示词末尾；总附加长度受 `cursor_rules_max_chars` 限制，超出会截断并写入提示。该能力可用于复用类似 Cursor Rule 的项目约束。

**上下文窗口**（`[agent]`）：每次向模型发请求前会压缩 `messages`——`tool_message_max_chars` 截断工具输出；`max_message_history` 限制条数；`context_char_budget > 0` 时按近似字符删最旧消息；`context_summary_trigger_chars > 0` 且总长超阈值时再调一次无 tools 的 API 生成「较早对话摘要」（尾部保留 `context_summary_tail_messages` 条）。REPL 长会话下裁剪会缩短本地消息列表；Web 单请求内工具多轮仍受益。

**终端历史加载（CLI REPL）**（`[agent] tui_load_session_on_start` / `AGENT_TUI_LOAD_SESSION_ON_START`）：默认 **`false`**，启动不读磁盘；设为 `true` 时从 `.crabmate/tui_session.json` 恢复会话（与 `--workspace` / `run_command_working_dir` 所指工作区一致）。此时 **`tui_session_max_messages`** 才限制加载条数（含 `system`），与上述「每次请求前」的上下文裁剪相互独立。当前 **REPL 不会**自动写回 `tui_session.json`（导出/保存能力保留在代码中供后续终端 UI 再接）。

**Web 对话任务队列**（`chat_queue_max_concurrent` / `chat_queue_max_pending`）：`POST /chat` 与 `POST /chat/stream` 经进程内有界队列调度，限制**同时执行**的 Agent 回合数与**排队**长度；队列满时返回 **503**，JSON 体含 `code: "QUEUE_FULL"`。`GET /status` 会返回 `chat_queue_running`、`chat_queue_completed_ok`、`chat_queue_completed_cancelled`、`chat_queue_completed_err`、`chat_queue_recent_jobs`（含 `cancelled` 标记），以及运行中任务的 **`per_active_jobs`**（PER 镜像：`awaiting_plan_rewrite_model`、`plan_rewrite_attempts`、`require_plan_in_final_content` 等；按队列 `job_id` 区分，**与浏览器会话无绑定**，完整「本会话是否在规划重写」需日后会话协议扩展）。多副本/跨进程需自行接外部消息队列（见 `docs/TODOLIST.md`）。

**工具执行性能**（`parallel_readonly_tools_max` / `AGENT_PARALLEL_READONLY_TOOLS_MAX`）：当模型在一轮内返回多只读、非构建锁类的 `SyncDefault` 工具时，服务端会并行调度；本项限制同时进入 **blocking 线程池** 的任务数，避免单次大批工具占满线程池。`get_current_time`、`convert_units` 等纯 CPU 工具在同路径下会在当前异步任务上**同步执行**（不经 `spawn_blocking`），减轻调度开销。`allowed_commands` 在内存中以共享 `Arc` 保存，正常 `run_command` 路径不再每轮整表克隆。

**与模型网关的 HTTP 连接**：进程内**一个**共享 `reqwest::Client`（连接池、空闲连接保留、TCP keepalive、`User-Agent`），多次调用 `chat/completions` 时可复用 **TLS/HTTP Keep-Alive**；协议仍是 HTTP（JSON 或 SSE），不是 WebSocket「单条长连接」。细节见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) 中 `http_client`。

常用模型 ID：`deepseek-chat`（默认）、`deepseek-reasoner`（推理链更长，适合复杂推理）。

## 基本使用（命令行与 Web）

### 快速开始

本地开发默认进入 REPL（需已设置 `API_KEY`）：

```bash
export API_KEY="your-api-key"
cargo run
```

### 命令行：子命令（推荐）

使用 `crabmate --help`、`crabmate help` 或 `crabmate help <子命令>`（与对应 `--help` 等价）查看完整说明。

| 子命令 | 作用 |
|--------|------|
| `serve [PORT]` | Web UI + HTTP API，默认端口 **8080**；`serve --host <ADDR>` 设置监听 IP（默认 `127.0.0.1`）。`--no-web` / `--cli-only` 仅 API。 |
| `repl` | 交互式终端对话；**不写任何子命令时默认进入 `repl`**。 |
| `chat` | 脚本化对话：见下文 **「`chat` 脚本与退出码」**；支持 `--query` / `--stdin` / `--user-prompt-file`、`--system-prompt-file`、`--messages-json-file`（整轮 messages）、`--message-file`（JSONL 多轮）、`--yes` / `--approve-commands`、`--output json`、`--no-stream`。 |
| `bench` | 批量测评：与下表 benchmark 选项相同（`--benchmark`、`--batch` 等）。 |
| `config` | 自检：检查配置、`API_KEY`、前端 `frontend/dist` 是否存在；`--dry-run` 可选（与不带该参数行为相同）。 |
| `doctor` | 一页本地诊断（Rust/npm/前端路径、`allowed_commands` 条数等，**脱敏**）；**不需要** `API_KEY`。 |
| `models` | `GET …/models` 列出模型 id（需 `API_KEY`；部分网关无此端点）。 |
| `probe` | 探测 `api_base` 上 models 端点连通性与 HTTP 状态（需 `API_KEY`）。 |

**全局选项**（写在子命令**之前**）：`--config <path>`、`--workspace <path>`、`--no-tools`、`--log <FILE>`。

**日志默认级别**：未设置 `RUST_LOG` 时，`serve` 默认 **info**；`repl` / `chat` / `bench` / `config` 默认 **warn**。需要 info 时请设置 `RUST_LOG` 或使用 `--log <FILE>`。

**兼容旧用法**：未写子命令时仍可使用原来的平铺参数（如 `--serve`、`--query`、`--benchmark`、`--dry-run`），程序会在内部映射为上述子命令，现有脚本一般无需修改。

### 常用命令行选项（兼容写法对照）

以下选项在 **推荐写法** 中归属子命令；仅在不写子命令的兼容模式下可直接放在程序名后。

| 选项              | 作用 |
|-------------------|------|
| `-h, --help`      | 显示命令行帮助。|
| `--config <path>` | 显式指定配置文件（全局，建议写在子命令前）。|
| `--serve [port]`  | 兼容：等价于 `serve [port]`。|
| `--host <ADDR>`   | 兼容：写在 `serve` 后为 `serve --host`；旧式 `--serve --host` 仍可用。|
| `--query <问题>`  | 兼容：等价于 `chat --query`。|
| `--stdin`         | 兼容：等价于 `chat --stdin`。|
| `--workspace <path>` | 全局：覆盖本次进程的初始工作区。|
| `--output <mode>` | 兼容：随 `chat`；`plain` / `json`。|
| `--no-tools`      | 全局：禁用工具。|
| `--no-web`        | 随 `serve`：仅 API。|
| `--cli-only`      | 同 `--no-web`。|
| `--dry-run`       | 兼容：映射为 `config` 自检（与 `config --dry-run` 相同）。|
| `--no-stream`     | 随 `repl` / `chat`。|
| `--log <FILE>`    | 全局：日志追加到文件并镜像 stderr。|

**Benchmark**（`bench` 子命令或兼容平铺）：

| 选项 | 作用 |
|------|------|
| `--benchmark <TYPE>` | `swe_bench`、`gaia`、`human_eval`、`generic`。|
| `--batch <FILE>` | 输入 JSONL。|
| `--batch-output <FILE>` | 输出 JSONL；默认 `benchmark_results.jsonl`。|
| `--task-timeout <SECS>` | 默认 `300`；`0` 不限制。|
| `--max-tool-rounds <N>` | `0` = 不限制。|
| `--resume` | 跳过输出中已有 `instance_id`。|
| `--bench-system-prompt <FILE>` | 从文件覆盖 system prompt。|

对应示例（**推荐子命令**；`crabmate` 可换为 `cargo run --`）：

```bash
# 使用默认配置交互运行（默认子命令 repl）
cargo run

# 指定配置文件后启动 Web（全局选项在子命令前）
cargo run -- --config /path/to/my.toml serve

# 将 debug 日志写入文件并同时打到 stderr
RUST_LOG=debug cargo run -- --log /tmp/crabmate.log repl

# Web 服务（默认 8080）
cargo run -- serve

# Web 服务（指定端口）
cargo run -- serve 3000

# Web + 初始工作区
cargo run -- --workspace /path/to/project serve 8080

# 局域网监听（注意安全）
cargo run -- serve --host 0.0.0.0

# 单次提问
cargo run -- chat --query "北京今天天气怎么样"

# 单次提问 JSON 输出
cargo run -- chat --output json --query "北京今天天气怎么样"

# 从标准输入读入问题
echo "1+1等于几" | cargo run -- chat --stdin

# 仅模型、无工具 + Web
cargo run -- --no-tools serve

# Benchmark：SWE-bench
cargo run -- bench --benchmark swe_bench --batch swebench_tasks.jsonl --batch-output results.jsonl --task-timeout 600

# Benchmark：GAIA / HumanEval / 续跑（略，同上将子命令改为 bench）
cargo run -- bench --benchmark gaia --batch gaia_tasks.jsonl --batch-output gaia_results.jsonl
cargo run -- bench --benchmark human_eval --batch humaneval_tasks.jsonl --batch-output humaneval_results.jsonl --task-timeout 60
cargo run -- bench --benchmark swe_bench --batch tasks.jsonl --batch-output results.jsonl --resume

# 配置自检（CI；与 `config --dry-run` 相同）
cargo run -- config
cargo run -- config --dry-run
```

以下 **旧写法** 仍有效（内部会映射为子命令）：

`cargo run -- --serve`、`cargo run -- --query "…"`、`cargo run -- --benchmark …`、`cargo run -- --dry-run` 等。

前端在 **`frontend/`** 目录（Vite + React + TypeScript + Tailwind CSS），需先构建后启动后端：

```bash
cd frontend && npm install && npm run build && cd ..
cargo run -- serve
```

后端从 `frontend/dist` 提供静态页面，API 与页面同源，无需 CORS。

- **GET /**：前端页面（聊天 + 工作区 + 状态栏），在浏览器打开即可对话。
- **POST /chat**：请求体 `message`（必填）、可选 `conversation_id`；可选 **`temperature`**（0～2，覆盖服务端默认）、可选 **`seed`**（整数，写入 `chat/completions` 的 `seed`）、可选 **`seed_policy":"omit"`**（本回合不带 `seed`，与 `seed` 互斥）。返回 `reply` 与 `conversation_id`。
- **POST /chat/stream**：流式对话（SSE）；除上述字段外还可选 **`approval_session_id`**；响应头 `x-conversation-id` 回传会话 ID。
- **POST /chat/approval**：Web 审批决策，请求体 `{"approval_session_id":"...","decision":"deny|allow_once|allow_always"}`。
- **GET /status**：返回当前模型、API 地址等后台状态。
- **GET /workspace**：返回当前工作目录路径及文件列表。
- **GET /health**：健康检查，返回 `{"status": "ok"}`。

**单次提问（脚本/管道）**：使用 **`chat --query`**、**`chat --stdin`** 或 **`chat --user-prompt-file`**（与 `--query`/`--stdin` 三选一）时，程序执行一轮 `run_agent_turn` 后退出。可选 **`--system-prompt-file`** 覆盖配置中的 system（不叠加工作区会话注入，与 Web 首轮 seed 语义一致）。**`--messages-json-file`** 提供单轮完整 `messages`（JSON 数组或 `{"messages":[...]}`）。**`--message-file`** 为 JSONL 批跑：每行 `{"user":"…"}` 在已有历史上追加用户消息并跑一轮，或 `{"messages":[...]}` 整表替换后再跑一轮；空行与 `#` 开头行跳过。

```bash
# 参数传入问题
cargo run -- chat --query "北京今天天气怎么样"

# 从标准输入读入问题（多行直到 EOF）
echo "1+1等于几" | cargo run -- chat --stdin

# CI：自动批准所有非白名单 run_command（仅可信环境）
cargo run -- chat --yes --query "…"

# 仅自动批准列出的命令名（逗号分隔，与 allowed_commands 合并匹配）
cargo run -- chat --approve-commands grep,git --query "…"
```

**`chat` 退出码**（便于 shell：`$?`）：**0** 成功；**1** 一般错误；**2** 用法/输入非法；**3** 模型接口或响应解析类失败；**4** 本回合内所有 `run_command` 均在审批中被拒绝（且发生过至少一次 `run_command`）；**5** 配额/限流等（典型 **HTTP 429**，以及文案中含 402/余额/503 等启发式归类）。模型错误信息经脱敏后写入 stderr。

运行后（交互模式）下，提示符为加粗着色的 **「我: 」**（青色）与助手行前 **「Agent: 」**（洋红），正文仍为 Markdown 着色；输入问题，例如：

- 「现在几点？」
- 「(123 + 456) * 2 等于多少？」
- 「北京今天天气怎么样？」
- 「今天几号？再帮我算 100 除以 5」

**REPL 内建命令**（以 `/` 开头，**不**发给模型）：`/help` 列出说明；`/clear` 清空历史（保留当前 `system`）；`/model` 查看 model、api_base、temperature、llm_seed；`/workspace` 显示当前工作目录，`/workspace <路径>` 或 `/cd <路径>` 切换到已存在的目录（工具 `run_command` 等随之在新工作区执行）；`/tools` 列出已加载工具名。**本地 shell 一行**（**不**写入对话、不调用模型）：在**交互终端**下，行首按 **`$`** 时输入提示由 **`我: `** 变为 **`bash#: `**，且 **`$` 不回显**，随后输入命令并回车即可（等价于旧式 **`$ <命令>`** 一行）；**管道/重定向等非 TTY 输入**时仍使用行首 **`$ <命令>`** 文本形式。在当前工作区执行**一行** shell（`sh -c` / Windows 为 `cmd /C`）；子进程 **stdin 关闭**（不适合 `vim` 等交互程序）。与模型工具 `run_command` 的白名单**无关**，相当于你在本机终端自行输入命令，**仅用于可信工作区**。启动时打印简要横幅（模型、工作区、工具数）；终端支持 ANSI 着色，设置环境变量 **`NO_COLOR`** 可关闭。

**`run_command` 终端审批**（REPL / `chat`）：若模型调用的命令**不在**配置白名单 `allowed_commands` 内，会在 stderr 打印待执行命令并等待 stdin 一行：**`y`**（或任意非 `n`/`a` 的文本，除空行）表示**允许本次**；**`a` / `always`** 表示**本会话内**永久允许该**命令名**（与 Web「永久允许」同语义，仅进程内）；**回车 / `n` / `q`** 等表示拒绝。白名单内命令不询问。**`chat --yes`** 跳过确认（**极危险，仅可信环境**）；**`chat --approve-commands a,b`** 将列出的命令名与配置白名单合并后再判断是否提示。

输入 `quit` / `exit` 或按 **Ctrl+D** 退出。

## 打包为 Debian `.deb` 包

本项目已内置 `cargo-deb` 的打包元数据，可在 Debian/Ubuntu 上打成 `.deb` 包后安装运行。

1. **安装 `cargo-deb` 子命令**（只需一次）：

   ```bash
   cargo install cargo-deb
   ```

2. **构建前端静态资源**（用于 Web 界面）：

   ```bash
   cd frontend
   npm install
   npm run build
   cd ..
   ```

3. **编译后端 Release 二进制**：

   ```bash
   cargo build --release
   ```

4. **生成 `.deb` 安装包**：

   ```bash
   cargo deb
   ```

   生成的安装包位于：

   ```bash
   ls target/debian/*.deb
   ```

5. **在系统中安装与卸载**：

   ```bash
   # 安装
   sudo dpkg -i target/debian/crabmate_0.1.0_amd64.deb

   # 如需卸载
   sudo apt remove crabmate
   ```

安装后可直接运行：

```bash
export API_KEY="your-api-key"
crabmate serve 8080
```

## 项目结构

项目代码结构与各模块机制请移步开发文档：

- `docs/DEVELOPMENT.md`

## 还可完善的方向

可从以下方向继续增强（按需实现）：

| 方向 | 说明 |
|------|------|
| **会话持久化** | 已支持：配置 `conversation_store_sqlite_path` 将 Web `conversation_id` 存 SQLite；多副本仍须外部存储（见 `TODOLIST`） |
| **配置外部化** | 通过环境变量或配置文件设置 `max_tokens`、`temperature`、白名单命令等 |
| **更多工具** | 如：读文件（受限路径）、搜索文件内容、当前目录下的 grep 等 |
| **安全** | run_command 可加「允许的工作目录」限制；或通过环境变量扩展白名单 |
| **日志与调试** | 可选记录请求/响应或仅工具调用，便于排查问题 |
| **代码结构** | 拆成多模块（如 `api.rs`、`tools.rs`）并为主流程和工具写单元测试 |

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
