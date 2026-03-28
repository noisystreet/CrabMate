# 配置说明

默认配置见仓库根目录 **`default_config.toml`**。可用 **`config.toml`** 或 **`.agent_demo.toml`** 覆盖，再被环境变量覆盖。示例片段见 **`config.toml.example`**。

## 环境变量（`AGENT_*`）

以下为常用项；**完整键名与默认值以 `default_config.toml` 为准**。

- **模型与 API**：`AGENT_API_BASE`、`AGENT_MODEL`、`AGENT_LLM_HTTP_AUTH_MODE`（`bearer` 默认，需 **`API_KEY`**；`none` 不向 `chat/completions` / `models` 发送 `Authorization`，本地 Ollama 等可不设 **`API_KEY`**）、`AGENT_SYSTEM_PROMPT`、`AGENT_SYSTEM_PROMPT_FILE`
- **温度与 seed**：`AGENT_TEMPERATURE`、`AGENT_LLM_SEED`
- **Web**：`AGENT_HTTP_HOST`（未传 `--host` 时生效）、`AGENT_WEB_API_BEARER_TOKEN`、`AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK`
- **工作区白名单**：`AGENT_WORKSPACE_ALLOWED_ROOTS`（逗号分隔；与 `[agent] workspace_allowed_roots` 等价）
- **Cursor 式规则**：`AGENT_CURSOR_RULES_ENABLED`、`AGENT_CURSOR_RULES_DIR`、`AGENT_CURSOR_RULES_INCLUDE_AGENTS_MD`、`AGENT_CURSOR_RULES_MAX_CHARS`
- **终答规划**：`AGENT_FINAL_PLAN_REQUIREMENT`（`never` / `workflow_reflection` / `always`）、`AGENT_PLAN_REWRITE_MAX_ATTEMPTS`
- **规划器模式**：`AGENT_PLANNER_EXECUTOR_MODE`（`single_agent` / `logical_dual_agent`）
- **分阶段规划**：`AGENT_STAGED_PLAN_EXECUTION`、`AGENT_STAGED_PLAN_PHASE_INSTRUCTION`、`AGENT_STAGED_PLAN_ALLOW_NO_TASK`、`AGENT_STAGED_PLAN_FEEDBACK_MODE`（`fail_fast` / `patch_planner`）、`AGENT_STAGED_PLAN_PATCH_MAX_ATTEMPTS`
- **对话队列**：`AGENT_CHAT_QUEUE_MAX_CONCURRENT`、`AGENT_CHAT_QUEUE_MAX_PENDING`
- **只读工具并行**：`AGENT_PARALLEL_READONLY_TOOLS_MAX`
- **单轮 `read_file` 缓存**：`AGENT_READ_FILE_TURN_CACHE_MAX_ENTRIES`（`0` 关闭；写类工具或工作区变更后会话内缓存整表清空）
- **`run_command` 白名单覆盖**：`AGENT_ALLOWED_COMMANDS`（逗号分隔）
- **MCP**：`AGENT_MCP_ENABLED`、`AGENT_MCP_COMMAND`、`AGENT_MCP_TOOL_TIMEOUT_SECS`
- **会话 SQLite**：`AGENT_CONVERSATION_STORE_SQLITE_PATH`
- **工作区备忘（首轮注入）**：`AGENT_MEMORY_FILE_ENABLED`、`AGENT_MEMORY_FILE`、`AGENT_MEMORY_FILE_MAX_CHARS`
- **项目画像（首轮注入）**：`AGENT_PROJECT_PROFILE_INJECT_ENABLED`、`AGENT_PROJECT_PROFILE_INJECT_MAX_CHARS`
- **工具解释卡**：`AGENT_TOOL_CALL_EXPLAIN_ENABLED`、`AGENT_TOOL_CALL_EXPLAIN_MIN_CHARS`、`AGENT_TOOL_CALL_EXPLAIN_MAX_CHARS`
- **长期记忆**：`AGENT_LONG_TERM_MEMORY_ENABLED`、`AGENT_LONG_TERM_MEMORY_SCOPE_MODE`、`AGENT_LONG_TERM_MEMORY_VECTOR_BACKEND`（默认 `fastembed`，可 `disabled`）、`AGENT_LONG_TERM_MEMORY_STORE_SQLITE_PATH`、`AGENT_LONG_TERM_MEMORY_TOP_K`、`AGENT_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK`、`AGENT_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX`、`AGENT_LONG_TERM_MEMORY_ASYNC_INDEX`、`AGENT_LONG_TERM_MEMORY_MAX_ENTRIES`、`AGENT_LONG_TERM_MEMORY_INJECT_MAX_CHARS`  
  Web 已配置 `conversation_store_sqlite_path` 时会话库与长期记忆可共用同一 SQLite；纯内存会话须单独配置 `long_term_memory_store_sqlite_path` 才能持久化记忆。CLI 默认路径为 `run_command_working_dir/.crabmate/long_term_memory.db`。若在 **repl / chat** 下启用长期记忆但打开库失败，进程会向 **stderr 打印一次性警告**（并继续运行，本进程内不注入记忆）；仍伴有 `crabmate` 目标下的 `warn` 日志。
- **联网搜索**：`AGENT_WEB_SEARCH_PROVIDER`、`AGENT_WEB_SEARCH_API_KEY`、`AGENT_WEB_SEARCH_TIMEOUT_SECS`、`AGENT_WEB_SEARCH_MAX_RESULTS`
- **`http_fetch`**：`AGENT_HTTP_FETCH_ALLOWED_PREFIXES`、`AGENT_HTTP_FETCH_TIMEOUT_SECS`、`AGENT_HTTP_FETCH_MAX_RESPONSE_BYTES`
- **上下文与工具消息**：`AGENT_MAX_MESSAGE_HISTORY`、`AGENT_TOOL_MESSAGE_MAX_CHARS`、`AGENT_TOOL_RESULT_ENVELOPE_V1`、`AGENT_MATERIALIZE_DEEPSEEK_DSML_TOOL_CALLS`、`AGENT_CONTEXT_CHAR_BUDGET`、`AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM`、`AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS`、`AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES`、`AGENT_CONTEXT_SUMMARY_MAX_TOKENS`、`AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS`
- **CLI REPL 会话文件**：`AGENT_TUI_LOAD_SESSION_ON_START`、`AGENT_TUI_SESSION_MAX_MESSAGES`
- **CLI 等待模型首包动效**（可选）：`AGENT_CLI_WAIT_SPINNER`（非空且非 `0`/`false` 即开启）。在 **`repl` / `chat`** 且为 **CLI 纯文本流式**（默认流式、非 `--no-stream`）时，于首段 reasoning/content 到达前在 **stderr** 显示 **indicatif** spinner 与已等待时间；**`NO_COLOR`** 或 **stderr 非 TTY** 时不启用。与 stdout 上的 **`Agent:`** 正文分离。

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

若存在 `workflow_validate_only` 结果，服务端还会按 `spec.layer_count` 约束规划步骤条数。

## 规划重写（`plan_rewrite_max_attempts`）

规划不合格时追加「请重写」的上限；超过后流式前端可能收到 `code: plan_rewrite_exhausted`。

## 逻辑双 agent（`planner_executor_mode = logical_dual_agent`）

先无工具规划轮，再执行器循环；planner 上下文会过滤 `role: tool` 正文。与 `staged_plan_execution` 并存时本模式优先。

## 分阶段规划（`staged_plan_execution`）

在 `planner_executor_mode = single_agent` 且开启时，每条用户消息先走无工具规划轮，再按 `steps` 执行。`no_task` + 空 `steps` 可跳过执行。规划 JSON 无法解析时降级为常规工具循环。API 调用通常多于关闭时。

**步级反馈（`staged_plan_feedback_mode`）**：默认 `fail_fast`（某步子循环 `Err` 或步内存在失败工具结果时，整轮计划按失败结束）。设为 `patch_planner` 时，会向规划器注入简短反馈并无工具重跑规划轮，将补丁 `steps` 与「当前步及之后」合并后继续执行（受 `staged_plan_patch_max_attempts` 限制，多耗 API）。

**CLI 规划轮终端输出（`staged_plan_cli_show_planner_stream`，默认 `true`，环境变量 `AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM`）**：仅影响 **REPL / `chat` 等 `out: None` 路径** 下，**无工具规划轮**与 **`patch_planner` 补丁规划轮**是否向 stdout 流式或整段打印模型原文（`Agent:` 前缀及正文）。设为 `false` 时这些轮次不在终端打印模型输出，仍保留 `staged_plan_notice` 队列摘要、分步注入 user 转录与后续执行步的助手输出；Web SSE 路径不受影响。

**规划步骤优化轮（`staged_plan_optimizer_round`，默认 `true`，环境变量 `AGENT_STAGED_PLAN_OPTIMIZER_ROUND`）**：在首轮 `agent_reply_plan` v1 解析成功且 `steps` 不少于 2 时，再追加一轮无工具请求，请模型合并**无数据依赖**的只读探查步，并提示在同一执行步内对「可同轮并行批处理」的内建工具（与执行层 `parallel_readonly_tools` 判定一致，不限于 `read_file`）发起多次调用。解析失败或用户取消优化轮时沿用首轮规划；成功则追加优化轮 assistant 并采用新 `steps`（多一次 API）。

## SyncDefault 工具 Docker 沙盒（`sync_default_tool_sandbox_mode`）

- **`none`（默认）**：与历史一致，在 Agent 进程内 `spawn_blocking` 执行 `HandlerId::SyncDefault` 工具。
- **`docker`**：每个 SyncDefault 工具调用经 **[bollard](https://docs.rs/bollard)** 走 **Docker Engine HTTP API** 创建并运行一次性容器（等价于 `docker run --rm -i`）：挂载当前工作区到容器内 `/workspace`（读写），只读挂载宿主 `crabmate` 到 `/crabmate`，在容器内运行 `crabmate tool-runner-internal`。**Linux/macOS** 默认连接本地 Unix 套接字（与 `docker` CLI 相同）；**`DOCKER_HOST`** 在部分环境下亦可由 bollard 解析。**默认网络隔离**（`network_mode: none`）；若设置非空的 **`sync_default_tool_sandbox_docker_network`**（如 `bridge`），则使用该网络以便容器内联网工具（如沙盒内的 `web_search`）可用。
- **`sync_default_tool_sandbox_docker_image`**：`docker` 模式**必填**（`finalize` 时非空校验）；镜像内需包含工具依赖（`git`、`rg`、`cargo` 等按实际启用工具准备），且**与宿主 `crabmate` 二进制同 CPU 架构**（或改为在镜像内安装 crabmate 而非挂载宿主二进制）。
- **`sync_default_tool_sandbox_docker_timeout_secs`**：单次容器生命周期等待上限（秒，默认 600），超时后 **force remove** 容器。
- **密钥**：临时 JSON 会写入宿主 `TMPDIR`（Unix 尝试 `0600`），含 `web_search_api_key` 等；仅在可信主机上使用。
- **不进入沙盒**：`run_command`、`workflow_execute`、`http_fetch`（独立 handler）、MCP 等仍按原路径在宿主执行。

环境变量：`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_MODE`、`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE`、`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK`、`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS`。

## 系统提示词

- **默认**：嵌入的 **`default_config.toml`** 使用 **`system_prompt_file = "prompts/default_system_prompt.md"`**，运行时读盘，**修改该 Markdown 无需重新编译**。
- **相对路径解析顺序**：进程**当前工作目录** → 各层**覆盖配置文件所在目录**（后加载的优先，如 `.agent_demo.toml` 先于 `config.toml`）→ **`run_command_working_dir`**（已规范化的工作区根）。**绝对路径**仅尝试该路径。
- **覆盖与优先级**：若某层 TOML **只写**内联 **`system_prompt`**、**不写**该层的 `system_prompt_file`，则会**取消**继承自更早层的 `system_prompt_file`，改为使用内联。环境变量阶段：**`AGENT_SYSTEM_PROMPT`** 会清除已合并的 `system_prompt_file`；随后若存在 **`AGENT_SYSTEM_PROMPT_FILE`** 则再设为文件路径（两者同时设置时以文件为准）。
- **finalize 阶段**：若仍存在 `system_prompt_file` 则读文件；否则使用非空内联；二者皆无则报错。

仓库内默认正文含工具与任务拆分等约定（例如**同一工作区路径在未被修改前不要重复 `read_file`**）。完全自定义时可改 `prompts/default_system_prompt.md` 或换用自有路径。

## Cursor-like 规则注入

`cursor_rules_enabled` 为真时读取 `cursor_rules_dir` 下 `*.mdc`（可附加 `AGENTS.md`），拼到系统提示词末尾，长度受 `cursor_rules_max_chars` 限制。

## 上下文窗口

请求前会压缩 `messages`：条数上限、`context_char_budget`、可选 LLM 摘要等。其中 **`tool_message_max_chars`**（`AGENT_TOOL_MESSAGE_MAX_CHARS`）：单条 `role: tool` 在**发往模型前**若超长则压缩；启用 **`tool_result_envelope_v1`** 时对 `crabmate_tool.output` 采用**首尾采样**并附带 `output_truncated` 等字段（见 **`docs/DEVELOPMENT.md`**）。详见 `default_config.toml`。

## Web 对话队列（`chat_queue_*`）

`/chat` 与 `/chat/stream` 经有界队列调度；满时 **503**、`QUEUE_FULL`。`/status` 返回队列与 `per_active_jobs` 等字段。

## 只读工具并行（`parallel_readonly_tools_max`）

限制同轮多只读工具进入 blocking 池的并发数： eligible 批含内建只读 **`SyncDefault`**、**`http_fetch`**（GET/HEAD）、**`get_weather`**、**`web_search`**（不含 **`http_request`**、**`run_command`**、MCP 等）。构建锁类（如 **`cargo_*`**、**`npm_*`**）整批降级为串行。

## HTTP 客户端

进程内共享 `reqwest::Client`（连接池、Keep-Alive）。细节见 **`docs/DEVELOPMENT.md`** 中 `http_client` 说明。

## 常用模型 ID

- `deepseek-chat`（默认）
- `deepseek-reasoner`（推理链更长）
