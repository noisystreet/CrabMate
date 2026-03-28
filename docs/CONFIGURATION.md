# 配置说明

默认配置见仓库根目录 **`default_config.toml`**。可用 **`config.toml`** 或 **`.agent_demo.toml`** 覆盖，再被环境变量覆盖。示例片段见 **`config.toml.example`**。

## 环境变量（`AGENT_*`）

以下为常用项；**完整键名与默认值以 `default_config.toml` 为准**。

- **模型与 API**：`AGENT_API_BASE`、`AGENT_MODEL`、`AGENT_SYSTEM_PROMPT`、`AGENT_SYSTEM_PROMPT_FILE`
- **温度与 seed**：`AGENT_TEMPERATURE`、`AGENT_LLM_SEED`
- **Web**：`AGENT_HTTP_HOST`（未传 `--host` 时生效）、`AGENT_WEB_API_BEARER_TOKEN`、`AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK`
- **工作区白名单**：`AGENT_WORKSPACE_ALLOWED_ROOTS`（逗号分隔；与 `[agent] workspace_allowed_roots` 等价）
- **Cursor 式规则**：`AGENT_CURSOR_RULES_ENABLED`、`AGENT_CURSOR_RULES_DIR`、`AGENT_CURSOR_RULES_INCLUDE_AGENTS_MD`、`AGENT_CURSOR_RULES_MAX_CHARS`
- **终答规划**：`AGENT_FINAL_PLAN_REQUIREMENT`（`never` / `workflow_reflection` / `always`）、`AGENT_PLAN_REWRITE_MAX_ATTEMPTS`
- **规划器模式**：`AGENT_PLANNER_EXECUTOR_MODE`（`single_agent` / `logical_dual_agent`）
- **分阶段规划**：`AGENT_STAGED_PLAN_EXECUTION`、`AGENT_STAGED_PLAN_PHASE_INSTRUCTION`、`AGENT_STAGED_PLAN_ALLOW_NO_TASK`、`AGENT_STAGED_PLAN_FEEDBACK_MODE`（`fail_fast` / `patch_planner`）、`AGENT_STAGED_PLAN_PATCH_MAX_ATTEMPTS`
- **对话队列**：`AGENT_CHAT_QUEUE_MAX_CONCURRENT`、`AGENT_CHAT_QUEUE_MAX_PENDING`
- **只读工具并行**：`AGENT_PARALLEL_READONLY_TOOLS_MAX`
- **`run_command` 白名单覆盖**：`AGENT_ALLOWED_COMMANDS`（逗号分隔）
- **MCP**：`AGENT_MCP_ENABLED`、`AGENT_MCP_COMMAND`、`AGENT_MCP_TOOL_TIMEOUT_SECS`
- **会话 SQLite**：`AGENT_CONVERSATION_STORE_SQLITE_PATH`
- **工作区备忘（首轮注入）**：`AGENT_MEMORY_FILE_ENABLED`、`AGENT_MEMORY_FILE`、`AGENT_MEMORY_FILE_MAX_CHARS`
- **项目画像（首轮注入）**：`AGENT_PROJECT_PROFILE_INJECT_ENABLED`、`AGENT_PROJECT_PROFILE_INJECT_MAX_CHARS`
- **工具解释卡**：`AGENT_TOOL_CALL_EXPLAIN_ENABLED`、`AGENT_TOOL_CALL_EXPLAIN_MIN_CHARS`、`AGENT_TOOL_CALL_EXPLAIN_MAX_CHARS`
- **长期记忆**：`AGENT_LONG_TERM_MEMORY_ENABLED`、`AGENT_LONG_TERM_MEMORY_SCOPE_MODE`、`AGENT_LONG_TERM_MEMORY_VECTOR_BACKEND`（默认 `fastembed`，可 `disabled`）、`AGENT_LONG_TERM_MEMORY_STORE_SQLITE_PATH`、`AGENT_LONG_TERM_MEMORY_TOP_K`、`AGENT_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK`、`AGENT_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX`、`AGENT_LONG_TERM_MEMORY_ASYNC_INDEX`、`AGENT_LONG_TERM_MEMORY_MAX_ENTRIES`、`AGENT_LONG_TERM_MEMORY_INJECT_MAX_CHARS`  
  Web 已配置 `conversation_store_sqlite_path` 时会话库与长期记忆可共用同一 SQLite；纯内存会话须单独配置 `long_term_memory_store_sqlite_path` 才能持久化记忆。CLI 默认路径为 `run_command_working_dir/.crabmate/long_term_memory.db`。
- **联网搜索**：`AGENT_WEB_SEARCH_PROVIDER`、`AGENT_WEB_SEARCH_API_KEY`、`AGENT_WEB_SEARCH_TIMEOUT_SECS`、`AGENT_WEB_SEARCH_MAX_RESULTS`
- **`http_fetch`**：`AGENT_HTTP_FETCH_ALLOWED_PREFIXES`、`AGENT_HTTP_FETCH_TIMEOUT_SECS`、`AGENT_HTTP_FETCH_MAX_RESPONSE_BYTES`
- **上下文与工具消息**：`AGENT_MAX_MESSAGE_HISTORY`、`AGENT_TOOL_MESSAGE_MAX_CHARS`、`AGENT_TOOL_RESULT_ENVELOPE_V1`、`AGENT_MATERIALIZE_DEEPSEEK_DSML_TOOL_CALLS`、`AGENT_CONTEXT_CHAR_BUDGET`、`AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM`、`AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS`、`AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES`、`AGENT_CONTEXT_SUMMARY_MAX_TOKENS`、`AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS`
- **CLI REPL 会话文件**：`AGENT_TUI_LOAD_SESSION_ON_START`、`AGENT_TUI_SESSION_MAX_MESSAGES`

```bash
export AGENT_MODEL=deepseek-reasoner
cargo run
```

## 配置文件示例

```toml
[agent]
api_base = "https://api.deepseek.com/v1"
model = "deepseek-reasoner"
# system_prompt = "…"
# system_prompt_file = "system_prompt.txt"
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

## 系统提示词

`system_prompt` 与 `system_prompt_file` 二选一（文件优先）；均未配置则启动报错。

嵌入的 **`default_config.toml`** 默认 `system_prompt` 中含一条工具使用约定：**同一工作区路径在未被修改前不要重复 `read_file`**，应直接引用对话历史中已有的 `role: tool` 结果（除非需核对行号或内容已变）。若你完全自定义 `system_prompt`，请自行决定是否保留类似表述。

## Cursor-like 规则注入

`cursor_rules_enabled` 为真时读取 `cursor_rules_dir` 下 `*.mdc`（可附加 `AGENTS.md`），拼到系统提示词末尾，长度受 `cursor_rules_max_chars` 限制。

## 上下文窗口

请求前会压缩 `messages`：工具输出截断、条数上限、`context_char_budget`、可选 LLM 摘要等。详见 `default_config.toml` 与 **`docs/DEVELOPMENT.md`**。

## Web 对话队列（`chat_queue_*`）

`/chat` 与 `/chat/stream` 经有界队列调度；满时 **503**、`QUEUE_FULL`。`/status` 返回队列与 `per_active_jobs` 等字段。

## 只读工具并行（`parallel_readonly_tools_max`）

限制同轮多只读 `SyncDefault` 工具进入 blocking 池的并发数。

## HTTP 客户端

进程内共享 `reqwest::Client`（连接池、Keep-Alive）。细节见 **`docs/DEVELOPMENT.md`** 中 `http_client` 说明。

## 常用模型 ID

- `deepseek-chat`（默认）
- `deepseek-reasoner`（推理链更长）
