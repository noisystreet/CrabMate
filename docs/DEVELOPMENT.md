# 开发文档（架构与模块说明）

本文面向**二次开发/维护**，重点解释各模块职责、关键机制与扩展点。  
若你只关心功能与使用方式，请看 **`README.md`**；**环境变量与配置细节**见 **`docs/CONFIGURATION.md`**，**子命令与 HTTP 路由**见 **`docs/CLI.md`**；**`chat` 退出码与 `--output json` 稳定行**见 **`docs/CLI_CONTRACT.md`**（与 **`docs/SSE_PROTOCOL.md`** 流错误码交叉引用）。

## TODOLIST 与功能文档约定

- **`docs/TODOLIST.md`**：只保留**未完成**项；**全局优先级（跨模块）**为 P0–P5 小节；**按源码模块**各独立成章（`agent/`、`llm/`、`tools/` 等）。实现某条后**从文件中删除该条目**（不要用 `[x]` 长期占位）；空的小节可删掉标题。历史追溯用 Git。
- **新功能 / 用户可见变更**（新 CLI 标志、HTTP 接口、配置键、工具名、Web/CLI 行为等）：合并代码时同步更新 **`README.md`**（面向使用者：功能、命令、配置、安全提示）和/或 **`docs/DEVELOPMENT.md`**（面向维护者：模块、协议、扩展点）。**内置工具**的能力说明与 Function Calling 示例见 **`docs/TOOLS.md`**，增删改工具时同步该文件。纯内部重构且无行为变化时，可只改 `DEVELOPMENT` 或注释。
- **Cursor 规则**：项目内 `.cursor/rules/todolist-and-documentation.mdc` 对 Agent 重申上述约定；**架构或 `src/` 模块组织变更**时另见 `.cursor/rules/architecture-docs-sync.mdc`（须同步更新本节「架构设计」与「代码模块索引」）。**前端**见 **`frontend-typescript-react.mdc`**；**`src/tools/` 增删改工具**见 **`tools-registry.mdc`**；**聊天 / SSE 协议与双端一致**见 **`api-sse-chat-protocol.mdc`**（`alwaysApply`）；**工作区 / 命令 / 拉取 URL 等安全敏感面**见 **`security-sensitive-surface.mdc`**；**依赖与许可证**见 **`dependencies-licenses.mdc`**。
- **PR / Issue**：仓库提供 **`.github/pull_request_template.md`** 与 **`.github/ISSUE_TEMPLATE/`**（可选模板）；发 PR 时可对照清单自检。
- **提交前检查**：根目录 `.pre-commit-config.yaml`（含 **`cargo fmt --all`**、**`cargo clippy --all-targets -- -D warnings`**、**commit-msg：Conventional Commits 校验**）。安装：`pip install pre-commit && pre-commit install`（配置里含 `default_install_hook_types: [pre-commit, commit-msg]` 时会一并安装 **pre-commit** 与 **commit-msg** 钩子；若本机是旧配置下已装过，请补跑 **`pre-commit install --hook-type commit-msg`**）。手动执行文件检查：`pre-commit run --all-files`（**不**会跑 commit-msg；说明校验仅在 **`git commit`** 时触发）。Cursor 规则 **`.cursor/rules/pre-commit-before-commit.mdc`** 要求 Agent 在 `git commit` 前先跑 pre-commit（无则至少 `fmt` + `clippy`）。改 `src/` 时另见 **`.cursor/rules/rust-clippy-and-tests.mdc`**（测试范围）、**`.cursor/rules/rust-error-handling.mdc`**（unwrap/unsafe）。配置/路由/环境变量与文档的对应关系见 **`.cursor/rules/todolist-and-documentation.mdc`** 中「Rust：默认配置、环境变量与 HTTP 路由」。
- **提交说明**：使用 **Conventional Commits**（`feat:` / `fix:` / `refactor:` 等），见 **`.cursor/rules/conventional-commits.mdc`**；本地由 **conventional-pre-commit** 钩子强制校验。

## 总览：系统由哪些部分组成

- **Rust 后端（`src/`）**：经 OpenAI 兼容 **`chat/completions`** 与配置中的 **`api_base`** 对接各供应商 LLM；实现 Agent 主循环、HTTP API（含 SSE 流式输出）、工具执行、工作区/任务/上传等能力。
- **Web 前端（`frontend/`）**：Vite + React + TS + Tailwind。负责聊天 UI、工作区浏览/编辑、任务清单、状态栏展示，以及消费后端 SSE 流。

## 架构设计

### 总体结构

CrabMate 在**单个 Rust 进程**内使用 **Tokio** 异步运行时：通过 **Axum** 暴露 HTTP，通过 **`runtime/`** 提供 CLI（含交互式终端与 **`chat` 单次**等），共享同一套 **Agent 回合**（`run_agent_turn` → **`agent::agent_turn`**）、**工具**（`tools`）与 **`AgentConfig`**。

### 逻辑分层（自外而内）

1. **接入层**：HTTP 路由与 chat/upload 等 handler（**`web/`**：`server`、`chat_handlers`）、`serve` 子命令启动 Web 与后台任务（`lib.rs::run`）、CLI 子命令与交互循环（`config::cli`、`runtime/cli` 等）。
2. **编排层**：Web 对话排队（`chat_job_queue`）、Agent 主循环与上下文/PER/工作流（**`agent/`**：`agent_turn`、`context_window`、`per_coord` 等）。
3. **模型层**：共享 HTTP 客户端（`http_client`）、请求拼装与重试（`llm`）、流式响应解析（**`llm::api`**，`stream_chat`）；上游错误体仅经 **`redact`** 截断后写入日志，避免整包进 `log` 输出或 `Err` 链。
4. **工具与工作流**：工具表驱动执行（`tools/mod.rs`）、按名分发与 Web 侧阻塞超时（`tool_registry`）、DAG 工作流（**`agent::workflow`**）。**`workflow_execute`** 解析节点时校验 **`tool_name`** 为内置工具名，并对 **`tool_args`** 做基于各工具 JSON Schema 的 **必填键**粗校验（递归进入数组元素中的对象；完整类型校验仍由各 `runner_*` 负责，见 **`tools/schema_check.rs`**）。**`workflow_execute`** 执行结果 JSON 含 **`workflow_run_id`**（与 `crabmate` 日志对齐）、**`trace`**（调度/节点尝试/重试退避事件）、**`completion_order`**；节点 **`max_retries`** 仅对超时 / `spawn_blocking` 汇合失败等**可重试**错误自动重跑（见 **`docs/TOOLS.md`**）。
5. **横向契约**：OpenAI 兼容类型（`types`）、SSE 控制面（**`sse/`**：`protocol` + `line` + **`mpsc_send`**）、工具结构化结果（`tool_result`）、配置（`config`）、Web 工作区/任务 API（`web/*`）。

```mermaid
flowchart TB
  subgraph entry [接入]
    WEB["Axum HTTP\n(web/)"]
    CLI["CLI\n(runtime)"]
  end
  subgraph agent [Agent 编排 agent/]
    Q[chat_job_queue]
    AT[agent_turn]
    CW["context_window +\nmessage_pipeline"]
    PC["per_coord +\nplan_artifact"]
    WRC[workflow_reflection_controller]
  end
  subgraph model [模型调用]
    HC[http_client]
    LLM[llm]
    BE[llm::backend]
    API[llm::api]
  end
  subgraph exec [工具与工作流]
    TR[tool_registry]
    TS[tools]
    WF[agent::workflow]
  end
  WEB --> Q
  Q --> AT
  CLI --> AT
  AT --> CW
  AT --> PC
  AT --> LLM
  LLM --> BE
  BE --> API
  API --> HC
  AT --> TR
  TR --> TS
  TR --> WF
```

### Web 流式对话数据流（概要）

1. 客户端 `POST /chat/stream` → **`ChatJobQueue`** 限流排队。  
2. **`run_agent_turn`** 携带 `messages` 与 `tools` 定义进入循环。  
3. **`llm`**（默认 **`llm::backend::OpenAiCompatBackend`** → **`llm::api::stream_chat`**）请求 `/chat/completions`（SSE），直到得到最终文本或 **`tool_calls`**（可注入自定义 **`ChatCompletionsBackend`**）。  
4. 若有工具调用 → **`agent_turn::per_execute_tools_common`**：若 **`tool_registry::tool_calls_allow_parallel_sync_batch`** 为真，则对整批 **只读** 且 **非 cargo/npm/前端等构建锁类** 的工具 **`buffer_unordered` 并行 `spawn_blocking`**（含 **`SyncDefault`**、**`http_fetch`**、**`get_weather`**、**`web_search`**；**`http_fetch`** 在并行 IO 前由 **`prefetch_http_fetch_parallel_approvals`** 串行完成白名单/审批，与串行路径语义一致；受 **`parallel_readonly_tools_max`**）；否则按序 **`tool_registry::dispatch_tool`** → **`tools::run_tool`**（或 **`agent::workflow`** 路径）→ 结果以 `role: "tool"` 写回 `messages`（顺序与模型给出的 `tool_calls` 一致）。**`read_file`** 在单轮内可走 **`ReadFileTurnCache`**（配置 **`read_file_turn_cache_max_entries`**）；写工具或 **`workspace_changed`** 后缓存清空。  
5. 控制面事件经 **`sse::protocol`**（`encode_message` / `SsePayload`）编码为 SSE 行下发前端。
6. 若请求携带 `conversation_id`（或服务端自动分配），回合结束后将 `messages` 写回会话存储（内存 `HashMap` 或 **`conversation_store_sqlite_path`** 配置的 SQLite），用于下次同会话延续；持久化前剥离 **`crabmate_long_term_memory`** 与 **`crabmate_workspace_changelist`** 注入条（后者由 **`workspace_changelist::sync_changelist_user_message`** 在每轮 P 步末尾刷新，仅进程内会话态）；**`agent_memory_file_enabled`** 时新会话首轮从工作区根读取备忘文件注入 `messages`（见 `src/agent_memory.rs`）。

### 上下文管道（观测）

同步消息管道（**`message_pipeline::apply_session_sync_pipeline`**）在每轮 **P** 步前改写会话侧 `messages`；**阶段顺序**与实现细节见 **`src/agent/message_pipeline.rs`** 模块文档及下文「**上下文窗口策略**」条目。

- **`GET /status`**：返回 **`message_pipeline_trim_count_hits`**、**`message_pipeline_trim_char_budget_hits`**、**`message_pipeline_tool_compress_hits`**、**`message_pipeline_orphan_tool_drops`**，均为**自进程启动以来**的累计命中次数（**非**单会话），用于对照配置是否实际触发了按条数/字符裁剪、工具正文压缩、孤立 `tool` 丢弃等逻辑。
- **日志**：**`RUST_LOG` 含 debug**（如 **`crabmate=debug`**）时 **`context_window`** 在每轮 P 步前打一行汇总 **`message_pipeline session_sync: …`**；更细可设 **`RUST_LOG=crabmate::message_pipeline=trace`**，每步一行 **`session_sync_step`**（`stage=`、`message_count=`、`non_system_chars_est=` 等），无需把全局日志开到 trace。
- **配置告警**：**`config::finalize`** 在 **`context_char_budget > 0`** 且 **`context_min_messages_after_system >= max_message_history`** 时 **`warn`**：按字符删旧消息在「条数上限已吃满」时往往难以生效，宜检查 **`AGENT_CONTEXT_*`** / **`max_message_history`** 等组合（键说明见 **`docs/CONFIGURATION.md`**）。

## `src/` 代码模块索引

> **维护约定**：增删 `lib.rs` 顶层 `mod`、调整目录/文件职责边界、或改变工具/路由/工作流的调用关系时，应同步更新**本节表格**与上文**架构设计**（含 Mermaid 是否与现状一致）。Cursor 规则见 **`.cursor/rules/architecture-docs-sync.mdc`**。

### 顶层模块（与 `src/lib.rs` 中 `mod` 声明一致）

| 路径 | 职责摘要 |
|------|----------|
| `agent/` | **`agent_turn/`**：主循环（Web + CLI 经 `run_agent_turn`）；子模块 **`messages`**（助手合并/分隔线）、**`staged_sse`**、**`params`**（`RunLoopParams`）、**`plan_call`**（P 步）、**`reflect`**（R）、**`execute_tools`**（E / `per_execute_tools_common`）、**`outer_loop`**、**`staged`**（分阶段与逻辑双 agent）；**`plan_optimizer`**：分阶段首轮规划后的可选无工具「步骤优化」user 文案与回复解析；**`plan_ensemble`**：`staged_plan_ensemble_count`>1 时的逻辑多规划员注入文案与合并轮解析；**`message_pipeline`**：`apply_session_sync_pipeline`（工具压缩→条数/字符裁剪→孤立 tool→合并 assistant）、`conversation_messages_to_vendor_body`（会话切片→供应商 `messages`：strip UI/长期记忆与 reasoning + `types::normalize_…`）；**`context_window`**：在管道之上做可选 LLM 摘要与 `prepare_messages_for_model` 入口；**`per_coord` / `plan_artifact` / `workflow_reflection_controller`**：PER 与终答规划；**`workflow/`**：DAG 工作流；`WorkflowApprovalMode::Interactive` 对应 Web SSE 审批通道。 |
| `chat_job_queue.rs` | Web `/chat`、`/chat/stream` 有界队列与并发上限；运行中任务的 `PerTurnFlight` 注册供 `GET /status` 的 `per_active_jobs`；入队参数为 **`StreamSubmitParams`**（流式）与 **`JsonSubmitParams`**（非流式 `/chat`）。 |
| `config/` | `AgentConfig`、编译嵌入 **`config/default_config.toml`** + **`config/session.toml`** + **`config/context_inject.toml`** + **`config/tools.toml`** + **`config/sandbox.toml`** + **`config/planning.toml`** + **`config/memory.toml`** + 用户文件 TOML、环境变量覆盖、`cli`（`parse_args`→`io::Result<ParsedCliArgs>` 具名字段、`normalize_legacy_argv`、`root_clap_command_for_man_page` 供 **`crabmate-gen-man`** 生成 **`man/crabmate.1`** 等）；默认 **`system_prompt_file`** 指向 **`config/prompts/default_system_prompt.md`**（运行时读盘，相对路径相对 cwd / 各配置文件目录 / `run_command_working_dir` 解析）；内联 **`system_prompt`** 在仅写内联时会清除继承的 `system_prompt_file`。加载后仍可按 `cursor_rules_*` / `AGENT_CURSOR_RULES_*` 拼接工作区规则文件。内部拆分为 `config/types.rs`（配置与枚举类型）、`config/source.rs`（TOML 段解析辅助）、`config/cursor_rules.rs`（规则文件收集与拼接）与 `config/workspace_roots.rs`（工作区根白名单解析），`mod.rs` 保留主装配流程。**长期记忆**：`long_term_memory_*` 与 `AGENT_LONG_TERM_MEMORY_*`（见 `README.md`）；嵌入默认见 **`config/memory.toml`**（**`long_term_memory_enabled = true`、向量后端默认 `fastembed`**（本地 ONNX）；**`disabled`** 为纯时间序检索）；`finalize` 仍拒绝 **`qdrant` / `pgvector`**（尚未接外部服务）。 |
| `http_client.rs` | 进程内共享 `reqwest::Client`（连接池、超时、keepalive）。 |
| `redact.rs` | 上游 HTTP 响应体等长文本的**日志预览截断**（`preview_chars` / `single_line_preview`），供 `llm::api`、`tools::web_search` 等使用。 |
| `text_encoding.rs` | 工作区文本字节解码：**`read_file` / `extract_in_file` / `GET /workspace/file`** 共用；支持显式 **`encoding`**（`utf-8` 严格、`utf-8-sig`、常见中文区编码、`utf-16`、**`auto`**：BOM 优先否则 **chardetng** 嗅探）；用 **`encoding_rs::decode_to_string_without_replacement`**，遇 **`DecoderResult::Malformed`** 返回明确错误，避免静默乱码。 |
| `text_sanitize.rs` | 用户可见正文轻量清洗（DSML 剥离、规划步骤描述自然化等）；**`materialize_deepseek_dsml_tool_calls_in_message(msg, enabled)`**：`enabled` 为 true 且不存在**可用的**原生 `tool_calls` 时，从 **`content` + `reasoning_content`** 中的 DeepSeek 风格 DSML 解析并写入 `Message.tool_calls`；**`llm::complete_chat_retrying`** 在每次成功 `stream_chat` 后按 **`AgentConfig::materialize_deepseek_dsml_tool_calls`** 调用；分阶段规划轮在丢弃网关误返回的原生 `tool_calls` 后**再次**按同一配置调用（仅从正文物化）。`enabled == false` 时不物化，强约束仅用 API `tool_calls`（与「仅一段 JSON」类结构化约定可并存为后续扩展）。 |
| `health.rs` | 与 `GET /health` 一致的运行状况报告（`build_health_report` 含 **`llm_http_auth_mode`**：`none` 时 **`api_key` 检查项可为空仍视为 ok**）；由 **`web::chat_handlers::health_handler`** 调用。 |
| `llm/` | **`mod`**：`ChatRequest` 构造、指数退避 **`complete_chat_retrying`**（入参含 **`ChatCompletionsBackend`**）；**`backend`**：可插拔 **`ChatCompletionsBackend`**，默认 **`OpenAiCompatBackend`**（委托 **`api::stream_chat`**）；**`api`**：`chat/completions` HTTP + SSE/JSON 解析、终端 Markdown（公式见 `runtime::latex_unicode`）；CLI **`plain_terminal_stream`** 下助手 **`reasoning_content`** / **`content`** 终端分色见 **`runtime::terminal_labels`**；可选首包等待动效（**`AGENT_CLI_WAIT_SPINNER`**）由 **`api::stream_chat`** 与 **`runtime::cli_wait_spinner`** 衔接（stderr **indicatif**，首段 plain 输出前清除）；**`openai_models`**：CLI `models`/`probe` 用的 **`GET …/models`** 请求与解析（终端不输出响应体原文）。 |
| `path_workspace.rs` | 工作区路径**单一真源**：`absolutize_relative_under_root`（工具相对路径）、`absolutize_workspace_subpath`（Web 可绝对/相对）、`ensure_canonical_within_root`（`Path::starts_with` 分量级前缀）、`resolve_web_workspace_read_path` / `resolve_web_workspace_write_path`（与 `file` 工具同边界）、`validate_effective_workspace_base`（每次 Web 请求重验当前工作区根仍在 `workspace_allowed_roots` 且非敏感前缀）、`is_sensitive_workspace_path` / `is_within_allowed_roots`。`canonical_workspace_root` 供 `file` / `markdown_links` / `exec` 复用。`canonicalize` 与 `open` 之间的 TOCTOU / symlink 竞态见模块注释与 `TODOLIST`。 |
| `runtime/` | `cli`：`chat`（`run_chat_invocation`，消费 **`config::cli::ChatCliArgs`**：文件 system/user、整表 **`--messages-json-file`**、JSONL **`--message-file`**、**`--yes` / `--approve-commands`**）与交互式 CLI；子命令 **`save-session`**（兼容别名 **`export-session`**；**`run_save_session_command`**，读 **`tui_session.json` 或 `--session-file`**，写 **`chat_export`** 与 Web 同形）、**`tool-replay`**（**`run_tool_replay_command`**：从会话提取 **`tool_replay`** fixture / 按 **`tools::run_tool`** 重放，见 **`runtime/tool_replay.rs`**）及交互式 CLI **`/save-session`**（同子命令）与 **`/export`**（当前内存消息）；交互式 CLI **`/doctor`** / **`/probe`** / **`/models`** 分别调用 **`cli_doctor::print_doctor_report`**、**`run_probe_cli`**、**`run_models_cli`**（与 **`crabmate doctor` / `probe` / `models`** 对齐）；**`cli_approval`**：**dialoguer** 于 TTY 下 **`run_command` / `http_fetch` / `http_request`** 非白名单审批（**stderr** 渲染；**`NO_COLOR`** 用朴素主题；管道/无头回退读一行 **`y`/`a`/`n`**）；**`cli_repl_ui::CliReplStyle`**：CLI 交互终端样式集中定义（欢迎横幅：FIGlet 风格 **CrabMate** ASCII（6 行）+ 模型/工作区与工具/内建命令/要点配置分节，消费 **`AgentConfig`** 与 **`no_stream`**；**`/config`**（`print_repl_config_summary`，与横幅同源字段+排障项，不含密钥）；`/help`、成功/错误行（行首 ✓/✗；**`NO_COLOR`** 或非 TTY 为 `[ok]`/`[err]`）；尊重 **`NO_COLOR`**、非 TTY 不着色）；**`cli_wait_spinner`**：可选 **`AGENT_CLI_WAIT_SPINNER`** 时 CLI 纯文本流式路径在首包前于 stderr 显示 spinner + 耗时（**`llm::api::stream_chat`** 内挂载）；**`repl_reedline::ReplSlashCompleter` + `ColumnarMenu`**：「我:」下 **`/`** 内建命令与 **`/export` / `/save-session`** 子参数的 **Tab** 补全（**`bash#:`** 时关闭）；**`cli_exit`**：**`CliExitError`** 与 `classify_model_error_message`（`main` 映射退出码 1–5）；向 `run_agent_turn` 传入 **`CliToolRuntime`**（**`auto_approve_all_non_whitelist_run_command`**、**`extra_allowlist_commands`**、**`CliCommandTurnStats`** 供 exit 4）以启用 **`run_command`** 非白名单 stdin 审批；交互式 CLI 支持 **`/clear`、`/model`、`/workspace`（含 `/cd`）、`/tools`、`/save-session`、`/export`、`/help`** 等行首内建命令（`classify_repl_slash_command` 单测），以及在工作区执行一行 shell（**`repl_reedline`**：**reedline** 于 **`spawn_blocking`** 内 **`read_repl_line_with_editor`**；TTY 下 **`CrabmatePrompt`** 左提示与 **`terminal_labels::{write_user_message_prefix, write_repl_bash_prompt_prefix}`** 同源并尊重 **`NO_COLOR`**；TTY 下**空缓冲**按 **`$`/`＄`** 即切换（**`DollarToggleEmacs`**）；仍兼容 **`$` + Enter**；历史 **`{run_command_working_dir}/.crabmate/repl_history.txt`**；非 TTY 仍 **`parse_repl_dollar_shell_line`** 行内 **`$ <命令>`**；**`run_repl_shell_line_sync`**；与 `run_command` 白名单无关，见 README），均不进入 `run_agent_turn`；**`cli_doctor`**：子命令 **`doctor` / `models` / `probe`**；`workspace_session`：**`initial_workspace_messages`**（CLI）；`terminal_labels` / `terminal_cli_transcript`；`plan_section`；**`benchmark`**；**`message_display`** / **`chat_export`** / **`latex_unicode`**。 |
| `sse/` | **`protocol`**：`SsePayload` / `encode_message`（根再导出）；**`line`**：`classify_agent_sse_line` 等（与 `frontend/src/sse_control_dispatch.ts` 语义对齐；当前无 crate 根再导出）；**`control_dispatch_mirror`**（`#[cfg(test)]`）：与前端 `classifySseControlPayloadParsed` 同序，金样 **`fixtures/sse_control_golden.jsonl`**；**`mpsc_send`**：`send_string_logged`；**`web_approval`**：Web 审批决策中文标签与 `timeline_log` 下发（`tool_registry` / 工作流共用）。 |
| `tool_registry.rs` | 按工具名选择 Workflow / 命令超时 / 天气与联网搜索超时 / 默认同步等策略；**`is_readonly_tool`** / **`tool_ok_for_parallel_readonly_batch_piece`** / **`tool_calls_allow_parallel_sync_batch`** / **`prefetch_http_fetch_parallel_approvals`** 供同轮安全并行判定（**`mcp__*`** 代理名视为非只读、且不参与并行批；**`http_request`** 等变更类 HTTP 亦不并行）。**`parallel_tool_wall_timeout_secs`** / **`execution_class_for_tool`**：并行只读批与 **`SyncDefault` + `spawn_blocking`** 的墙上时钟与串行 **`dispatch_tool`**、各 **`execute_*_web`** 的 **`tokio::time::timeout`** 秒数一致。**`CliToolRuntime`**：`run_command` CLI 路径的审批、**`--yes`/`--approve-commands`** 自动批准与 **`CliCommandTurnStats`**（`agent_turn` 每回合开头 **`reset_command_stats`**）。**`dispatch_tool`** 入口可选 **`tool_call_explain`**：非只读工具要求 JSON 顶层 **`crabmate_explain_why`** 后剥离再执行；MCP 仅剥离不要求。**Docker 沙盒**（**`sync_default_tool_sandbox_mode = docker`**）：**`SyncDefault`** 经 **`run_sync_default_in_docker`**；**`RunCommand` / `RunExecutable` / `GetWeather` / `WebSearch` / `HttpFetch` / `HttpRequest`** 在各自 **`execute_*`** 路径完成审批与白名单解析后，经 **`dispatch_non_sync_tool_to_docker`** → **`tool_sandbox::run_tool_in_docker`**（**`write_runner_config_json_with_allowed_commands`** 用于 **`run_command`** 审批扩展白名单）；否则保持 **`spawn_blocking`** 等原路径。 |
| `tool_sandbox/` | 沙盒子模块：**`backend`**（`SyncDefaultSandboxBackend` trait + `SandboxRunRequest`，含 **`user`** → bollard **`Config.user`**）；**`docker_bollard`**（默认 **bollard** 实现）；**`runner`**（`SandboxToolRunnerConfig` 临时 JSON、`tool_runner_internal_main` 按 **`ToolInvocationLine.kind`** 分派）。**`run_tool_in_docker`** / **`run_sync_default_in_docker`** 组装请求并经全局 **`SANDBOX_BACKEND`** 执行。面向使用者的启用步骤、镜像与网络说明见 **`docs/CONFIGURATION.md`**「SyncDefault 工具 Docker 沙盒」。 |
| `tool_call_explain.rs` | **`require_explain_for_mutation`** / **`strip_explain_why_if_present`** / **`annotate_tool_defs_for_explain_card`**（`lib.rs::run` 在 `build_tools()` 后为副作用工具追加描述说明）。 |
| `tool_result.rs` | 工具输出的结构化 `ToolResult` 与旧式字符串兼容；**`crabmate_tool` v1 信封**（`encode_tool_message_envelope_v1` / `tool_message_payload_for_inner_parse` / `maybe_compress_tool_message_content`）供写入模型上下文与再解析；信封与 SSE **`tool_result`** 均含 `tool_call_id`、`execution_mode`、`parallel_batch_id`（并行只读批）、失败时 **`retryable`**（启发式）。 |
| `tools/` | 全部 Function Calling 定义、`ToolContext`、`run_tool`；`tools/mod.rs` 与 `tools/markdown_links.rs` 的测试已外移到同名子目录 `tests.rs`，并把工具调用摘要逻辑拆到 `tools/tool_summary.rs`，降低主文件长度；子模块见下表。 |
| `types.rs` | `Message`、`Tool`、流式 chunk 等 OpenAI 兼容类型；`Message::system_only` / `user_only`、`messages_chat_seed` 供 Web 首轮与 CLI 共用。**`is_message_excluded_from_llm_context_except_memory`**：合并 `is_chat_ui_separator` 与 `is_chat_timeline_marker`，供上下文摘要 / 分阶段规划等过滤。**`messages_for_api_stripping_reasoning_skip_ui_separators`** 与 **`normalize_messages_for_openai_compatible_request`**：由 **`agent::message_pipeline`** 的出站函数组合调用（`conversation_messages_to_vendor_body` / `normalize_stripped_messages_for_vendor_body`）。 |
| `conversation_store.rs` | Web 会话可选 **SQLite**：`conversation_id` → `messages` JSON + `revision` + `updated_at_unix`；TTL/条数上限与内存模式一致；`SaveConversationOutcome` 定义于此；按 revision 条件更新 JSON 的共性在 **`update_messages_json_if_revision`**。 |
| `long_term_memory_store.rs` | 长期记忆表 **`crabmate_long_term_memory`**（`scope_id`、正文、`embedding` BLOB）；与会话库可同文件。 |
| `long_term_memory.rs` | 每轮在 `prepare_messages_for_model` 前注入 `user` 条（`name=crabmate_long_term_memory`，**不**发往上游：`llm` 构造请求时过滤）；Web 成功后异步索引 user/assistant 终答；CLI 用 `run_command_working_dir/.crabmate/long_term_memory.db` 或 `long_term_memory_store_sqlite_path`。 |
| `mcp/mod.rs` | **MCP 客户端（stdio）**：`run_agent_turn` 开头可选 `try_open_session_and_tools`（`rmcp` + `TokioChildProcess`），按 **`mcp_enabled` + `mcp_command` 指纹** 在**进程内复用**同一条 stdio 连接（避免每轮重启子进程）；将远端 `tools/list` 映射为 OpenAI `Tool`（`mcp__{slug}__{name}`）并与内建列表合并；执行经 `tool_registry::dispatch_tool` → `tools/call`（超时 `mcp_tool_timeout_secs`，输出按 `command_max_output_len` 截断）。**安全**：`mcp_command` 显式允许启动子进程，须可信配置源；**未**复用 `run_command` 白名单。当前仅 stdio；HTTP/SSE 传输、资源/采样、将本进程暴露为 MCP server 等仍为后续方向。 |
| `agent_memory.rs` | 工作区相对路径备忘文件读取（`load_memory_snippet`）；与 **项目画像**、**依赖结构摘要** 合并后首轮消息组装在 **`project_profile::build_first_turn_user_context_markdown`**（Web **`build_messages_for_turn`**、CLI **`prepend_cli_first_turn_injection`**、CLI 路径 **`workspace_session::initial_workspace_messages`**）。 |
| `project_profile.rs` | **项目画像**：只读扫描 `Cargo.toml` / `package.json` / 顶层目录 / **tokei** 语言占比 / 可选 **`cargo metadata --no-deps`**，生成 Markdown；Web **`GET /workspace/profile`**；首轮与备忘、**`project_dependency_brief`** 合并见 **`build_first_turn_user_context_markdown`**（**`project_profile_inject_*`**）。 |
| `project_dependency_brief.rs` | **依赖结构摘要**：工作区内执行 **`cargo metadata`**（完整 resolve，**非** `--locked`），从 **`resolve.nodes[].deps`** 提取 **workspace 成员包之间**的边，输出 **Mermaid**（`flowchart LR`，节点/边上限制）与 **JSON**（`crabmate_project_dependency_brief_version` + `cargo` / `npm`）；npm 为根与 `frontend/package.json` 的依赖**名**节选。首轮注入预算 **`project_dependency_brief_inject_*`**。 |
| `read_file_turn_cache.rs` | 单轮 **`run_agent_turn`** 内 **`read_file`** 结果缓存（键：canonical 路径 + 行区间等；校验 **mtime + size**）。**`execute_tools`** 在任意非只读工具执行后或 **`workspace_changed`** 时 **`clear`**，避免脏读。容量 **`read_file_turn_cache_max_entries`**（`0` 关闭）；嵌入方可选传入 **`RunAgentTurnParams::read_file_turn_cache`** 覆盖默认句柄。 |
| `workspace_changelist.rs` | **会话级**工作区写入追踪：按作用域键（**`long_term_memory_scope_id`**；Web 为 **`conversation_id`**；无则为 **`__default__`**）在 **`create_file` / `modify_file` / `copy_file` / `move_file` / `delete_file` / `append_file` / `search_replace` / `apply_patch` / `structured_patch`** 成功写盘后累积相对路径与「本会话首次触碰」基线；**`prepare_messages_for_model`** 在可选 LLM 摘要**之后**注入 **`user.name=crabmate_workspace_changelist`**（unified diff 摘要，受 **`session_workspace_changelist_max_chars`** 约束）。**`workflow_execute` 节点**内工具经独立 **`ToolContext`**，**不**写入此表。 |
| `web/` | Web（HTTP）专用 axum 模块：`app_state`（`AppState`、`ConversationBacking`：内存或 SQLite、可选 **`long_term_memory`**、**`web_tasks_by_workspace`**：侧栏任务清单按工作区键入的进程内表；**`cfg`** 为 **`Arc<RwLock<AgentConfig>>`** 供 **`POST /config/reload`** 与 handler 读快照；**`config_path_for_reload`** 与启动时 **`--config`** 对齐；SQLite 路径下 **`save` / `truncate`** 经统一 **`sqlite_conversation_store_op`** 包装）、`tasks_types`（`TasksData` / `TaskItem`）、`chat_handlers`（`/chat*`、`/chat/branch`、`/config/reload`、`/upload*`、`/health`、`/status`；**`CONVERSATION_CONFLICT_*`** 与 **`conversation_conflict_sse_line`** 供 HTTP 与 SSE 冲突文案一致）、`server`（Router 组装；Bearer 中间件是否在启动时挂载由 **`web_api_bearer_layer_enabled`** 决定，热重载不切换该层）、`workspace`（含 **`GET /workspace/profile`** 项目画像）、`task`（`/tasks` 读写内存表）。`open_conversation_sqlite` 会 **`LongTermMemoryRuntime::migrate_on_connection`**；`AppState` 由 `lib.rs::run` 装配；`SaveConversationOutcome` 在 **`conversation_store`**，crate 根再导出供 `chat_job_queue` 等使用。 |

### `lib.rs` 额外职责（非独立文件但需知）

- `run()` 中创建 `AppState`、监听地址与清理任务，Router 组装下沉到 `web::server::build_app`（chat、status、health、workspace、tasks、upload、静态前端 `dist` 等）。
- **`AppState`**：定义于 **`web::app_state`**，`Arc` 持有 **`SharedAgentConfig`**（**`Arc<RwLock<AgentConfig>>`**）、共享 `reqwest::Client`、工作区覆盖路径、上传目录、对话队列、**`ConversationBacking`**（内存或 SQLite）、**`web_tasks_by_workspace`**（`RwLock<HashMap<workspace_path, TasksData>>`，**不**落盘）等；**`chat_job_queue`** 在每轮 **`run_agent_turn`** 前 **`read`+`clone`** 得快照 `Arc<AgentConfig>`，避免长跑任务与热重载互相撕裂；crate 根 `pub(crate) use` 保持 `chat_job_queue` / `web/workspace` 等路径不变。
- **`RunAgentTurnParams`**：库根 `run_agent_turn` 的唯一入参（Web / CLI / benchmark 共用），避免长形参列表。可选 **`llm_backend: Option<&dyn ChatCompletionsBackend>`**（`None` 时与历史一致，使用 **`llm::default_chat_completions_backend()`** / **`OPENAI_COMPAT_BACKEND`**），便于嵌入方接入自建网关而不改 Agent 主循环。另含 **`temperature_override` / `seed_override`**（与 Web `POST /chat*` 对齐；摘要路径仍固定低温且无 seed）。**`cli_tool_ctx: Option<&CliToolRuntime>`**：终端模式下传入时，**`run_command`** 若命令不在 `allowed_commands`（及 CLI 额外允许列表），经 stdin 交互确认，或 **`chat --yes`** 全放行、**`--approve-commands`** 按名放行（与 Web SSE 审批语义：`y`≈AllowOnce，`a`≈AllowAlways，进程内 `persistent_allowlist`）；Web 队列传 `None`。`agent_turn` 入口对 CLI 上下文调用 **`reset_command_stats`** 以便按回合统计拒绝次数。
- **`CliExitError`**：crate 根 `pub use`；`main` 对 `run()` 的 `Err` **downcast** 后按 **`code`** 调用 **`process::exit`**（0 成功；1 一般；2 用法；3 模型；4 本回合全部 `run_command` 被拒；5 配额/限流启发式）。详见 **`README.md`**「`chat` 退出码」。

### `src/tools/` 子文件（实现域一览）

与 `tools/mod.rs` 中 `mod` 声明保持一致；新增工具文件时请在此**增行**。

| 文件 | 职责域 |
|------|--------|
| `calc.rs` | 数学表达式（`bc`） |
| `unit_convert.rs` | `convert_units`：基于 [`uom`](https://crates.io/crates/uom) 的长度/质量/温度/信息量/时间/面积/压强/速度换算 |
| `cargo_tools.rs` | Cargo 子命令封装（含 `cargo_outdated` / `cargo_machete` / `cargo_udeps` 等） |
| `ci_tools.rs` | 本地 CI / 流水线类工具 |
| `code_metrics.rs` | 代码度量与分析：`code_stats`（tokei/cloc/内置行数统计）、`dependency_graph`（Cargo/Go/npm 依赖图，Mermaid/DOT）、`coverage_report`（LCOV/Tarpaulin/Cobertura 覆盖率解析） |
| `code_nav.rs` | 代码导航、文件大纲等 |
| `command.rs` | `run_command` 白名单与进程执行；可选 `cargo test …` 路径经 **`test_result_cache`** |
| `test_result_cache.rs` | 进程内 LRU：**`cargo_test` / `rust_test_one`**、**`npm_run` `script=test`**、**`run_command` `cargo`+`test`**（无 `--nocapture`/`--test-threads`）；指纹为工作区内 `.rs`/`.toml`/`Cargo.lock`（Rust）或 `package.json`/lock（npm）的 mtime+size |
| `package_query.rs` | `package_query`：apt/rpm 只读包查询（安装状态/版本/来源统一抽象） |
| `debug_tools.rs` | 调试辅助类工具 |
| `diagnostics.rs` | `diagnostic_summary`：脱敏环境/工具链/工作区路径摘要 |
| `error_playbook.rs` | `error_output_playbook`：对已脱敏错误输出做启发式归类，并给出经 `allowed_commands` 过滤的 `run_command` 建议字符串（不执行）；`playbook_run_commands`：同上启发式后依次执行建议命令（内部 `command::run`） |
| `dev_tag.rs` | Development 子域标签：`tags_for_tool_name`、`suggest_dev_tags_for_workspace`（供 `build_tools_with_options` 过滤）；标签含 `general`/`rust`/`frontend`/`python`/`cpp`/`vcs`/`quality`/`go`/`security`/`shell`/`docker` |
| `exec.rs` | `run_executable` |
| `file/` | 工作区文件工具目录：`mod.rs` 再导出各 `pub fn`；`path`（`resolve_for_read`/`resolve_for_write` 等）、`write_ops`、`read_tool`、`directory`、`tree_glob`、`inspect`、`extract`、`mutate`、`perm`、`symlink`、`display_fmt`；单元测试 `tests.rs` |
| `format.rs` / `lint.rs` | 格式化（Rust/Python/C++/Go/Shell/JS·TS/Markdown/YAML/XML/SQL + `prettier`）与 lint 聚合 |
| `frontend_tools.rs` | 前端 npm 脚本类 |
| `git.rs` | Git 只读查询（status/diff/log/blame 等）与受控写入（stage/commit/checkout/push/merge/rebase/stash/tag/reset/cherry-pick/revert 等） |
| `go_tools.rs` | Go 工具链：`go build`/`test`/`vet`/`mod tidy`/`gofmt -l`/`golangci-lint` |
| `grep.rs` / `symbol.rs` | 工作区内文本搜索、Rust 符号 |
| `nodejs_tools.rs` | Node.js 生态：`npm install`/`npm run`/`npx`/`tsc --noEmit` |
| `spell_astgrep_tools.rs` | `typos_check`、`codespell_check`（拼写，只读；支持项目词典参数）、`ast_grep_run`（结构化搜索）、`ast_grep_rewrite`（结构化改写，默认 dry-run，写盘需 confirm） |
| `markdown_links.rs` | `markdown_check_links`：Markdown 相对链接 + `#fragment` 锚点检查，支持 text/json/sarif 输出，可选外链前缀 HEAD（同 URL 去重） |
| `structured_data.rs` | `structured_validate` / `structured_query` / `structured_diff` / `structured_patch`：JSON·YAML·TOML·CSV·TSV 校验、路径查询、结构化 diff；以及 JSON/YAML/TOML 的定点补丁（默认 dry-run） |
| `table_text.rs` | `table_text`：CSV/TSV 等分隔文本的预览、列数校验、列筛选与聚合（与 `structured_*` 互补） |
| `tool_summary.rs` | `summarize_tool_call`：将各工具入参映射为 Web/SSE/TUI/CLI 共用的**英文**简短摘要（`ToolSpec.summary` 的 `Static` 字符串亦同） |
| `tool_params/` | 各工具 JSON Schema（`params_*`）按领域拆分子文件，`mod.rs` 再导出；`pub(in crate::tools)` 以便与 `tool_specs_registry` 同层引用 |
| `schema_check.rs` | **`workflow_tool_args_satisfy_required`**：对照内置工具 parameters schema 检查 `tool_args` 是否含 **required** 键（工作流解析阶段粗校验） |
| `tool_specs_registry/` | `tool_specs()`：`specs/*.inc.rs` 为数组字面量，`include!` 载入后在 `OnceLock` 中拼接为 `&'static [ToolSpec]`（name/description/category/parameters/runner） |
| `text_transform.rs` | `text_transform`：纯内存 Base64/URL 编解码、短哈希、按行合并与按分隔符切分（不落盘，有长度上限） |
| `text_diff.rs` | `text_diff`：两段 UTF-8 文本或工作区内两文件的行级 unified diff（与 Git 无关，输出可截断） |
| `patch.rs` | unified diff 应用 |
| `precommit_tools.rs` | `pre-commit run` 封装（依赖 `.pre-commit-config.yaml`） |
| `process_tools.rs` | 进程与端口管理（只读）：`port_check`（ss/lsof）、`process_list`（ps 过滤） |
| `python_tools.rs` | Python：`ruff check`、`python3 -m pytest`、`mypy`、`uv sync` / `uv run`、可编辑安装（uv / pip）；供 `format`（`.py` 的 ruff format）、`lint`、`quality_workspace`、`ci_pipeline_local` 调用 |
| `quality_tools.rs` | 工作区质量组合检查 |
| `release_docs.rs` | `changelog_draft`（git log → Markdown 草稿）、`license_notice`（cargo metadata → 许可证表） |
| `repo_overview.rs` | `repo_overview_sweep`：可选 **项目画像**（`project_profile::build_project_profile_markdown`，与 Web 首轮注入同源）+ 文档预览 + 源码树 + 构建/清单路径 glob 汇总（只读聚合；结论由模型撰写） |
| `docs_health_sweep.rs` | `docs_health_sweep`：文档预览 + typos + codespell + `markdown_check_links`（外链 HEAD 不经 http_fetch 审批） |
| `rust_ide.rs` | 编译器 JSON、rust-analyzer LSP（goto/references/hover/documentSymbol 等） |
| `schedule.rs` | 提醒与日程持久化 |
| `security_tools.rs` | 安全审计类 |
| `source_analysis_tools.rs` | 源码分析工具：`shellcheck_check`（Shell 脚本静态分析）、`cppcheck_analyze`（C/C++ 静态分析）、`semgrep_scan`（多语言 SAST）、`hadolint_check`（Dockerfile lint）、`bandit_scan`（Python 安全分析）、`lizard_complexity`（圈复杂度） |
| `time.rs` / `weather.rs` / `web_search.rs` | 时间、天气（Open-Meteo）、联网搜索（Brave/Tavily） |
| `http_fetch.rs` | `http_fetch`（GET/HEAD）与 `http_request`（POST/PUT/PATCH/DELETE + 可选 JSON body）；共享重定向记录、体长上限与 `http_fetch_allowed_prefixes` 的**同源 + 路径前缀边界**校验；**Web 流式 + 审批会话**与 **CLI** 下二者未匹配前缀均可审批（`http_request` 永久键含 **METHOD**）；**`workflow_execute` 同步节点**仍仅白名单 |

## 核心机制：Agent 主循环与工具调用

核心流程在 `src/lib.rs` 的 `run_agent_turn(RunAgentTurnParams { … })`：内部组装 **`RunLoopParams`** 后调用 **`run_agent_turn_common`**（实现见 **`src/agent/agent_turn.rs`**）。

- **MCP（可选）**：`[agent] mcp_enabled` + 非空 `mcp_command` 时，`run_agent_turn` 在回合开头尝试连接 stdio MCP server；成功则 **`RunLoopParams::mcp_session`** 持有 `Arc<Mutex<RunningService<…>>>`，并将合并后的工具表作为 **`tools_defs`**。失败则打 `warn` 并仅使用内建工具。配置键与环境变量见 **`README.md`**（`AGENT_MCP_*`）。
- **输入**：构造 `ChatRequest`（`src/types.rs`）并携带 `tools`（Function Calling 定义）。
- **P（命名上的「规划」步）**：`per_plan_call_model_retrying(PerPlanCallModelParams { … })` —— **一次** `stream_chat`，由模型产出正文或 `tool_calls`，并非独立规划器。
- **调用模型**：默认经 **`llm::OpenAiCompatBackend`** 调用 **`src/llm/api.rs`** 的 `stream_chat`（`POST {api_base}/chat/completions`）；`stream: true`（SSE 增量）。CLI `--no-stream` 或 `RunAgentTurnParams { no_stream: true, … }` 时为 `stream: false`，按 OpenAI 兼容 `ChatResponse` 解析 `choices[0].message`（有正文则经 `out` 整段下发）。**DSML 物化**在 **`llm::complete_chat_retrying`** 成功返回后执行（见 **`materialize_deepseek_dsml_tool_calls`**），不在 `stream_chat` 内。**自定义后端**：实现 **`llm::ChatCompletionsBackend`** 并在 **`RunAgentTurnParams { llm_backend: Some(&your_backend), … }`** 中传入；须保持与现有 `Message` / `tool_calls` / SSE `out` 语义一致。其它协议形态可在该 trait 内适配。**CLI 终端输出**：`RunAgentTurnParams { plain_terminal_stream: true, … }`（仅 `runtime::cli`）时 `render_to_terminal && out.is_none()` 下助手为纯文本流式/整段（**`reasoning_content`** 与 **`content`** 分色，不经 `message_display` 剥规划 JSON）；Web 队列等传 `plain_terminal_stream: false`，`out.is_none()` 时仍可用 `markdown_to_ansi`（避免污染服务端 stdout 的误用）。
- **分阶段规划**（`[agent] staged_plan_execution` / `AGENT_STAGED_PLAN_EXECUTION`）：为 true 时 `run_agent_turn_common` 先走规划轮（`llm::no_tools_chat_request_from_messages`，在 `agent_turn::staged` 内拼好消息并剥离 `reasoning_content` 后构造请求；语义与 `tools: []` + `tool_choice: "none"` 的禁止工具调用一致），解析 `agent_reply_plan` v1 后按 `steps` 顺序多次进入外层 Agent 循环。**`staged_plan_optimizer_round`**（默认 true，`AGENT_STAGED_PLAN_OPTIMIZER_ROUND`）：当 `steps.len() >= 2` 时，在 `send_staged_plan_started` 之前再跑一轮无工具优化（`prepare_staged_planner_no_tools_request` + `complete_chat_retrying`），注入 `plan_optimizer::staged_plan_optimizer_user_body`（内含本会话 **`tools_defs`** 中经 **`tool_ok_for_parallel_readonly_batch_piece`** 筛出的可同轮并行内建工具名）；解析成功则追加优化 assistant 并替换 `steps`，失败或用户取消则回退首轮规划（取消时弹出优化 user 以免孤立上下文）。**`staged_plan_ensemble_count`**（默认 1，钳制 1–3，`AGENT_STAGED_PLAN_ENSEMBLE_COUNT`）：大于 1 时在首轮 assistant 已入史后、`send_staged_plan_started` 与优化轮之前，`maybe_run_staged_plan_ensemble_then_merge` 串行调用无工具规划（`plan_ensemble` 注入的 B/C 角色 user；**辅助规划员 assistant 不入会话**），再跑合并轮（合并 assistant 入史）；某辅助轮解析失败则停止追加；有效草案不足 2 份则跳过合并。若 JSON 中 **`no_task: true` 且 `steps` 为空**，表示模型判定用户无具体可拆任务：`run_staged_plan_with_prepared_request` **不**发分步 SSE，将规划轮 assistant **追加**后直接 **`run_agent_outer_loop`**。**`staged_plan_allow_no_task`**（默认 true，`AGENT_STAGED_PLAN_ALLOW_NO_TASK`）：仅影响内置规划说明是否包含 `no_task` 约定；为 false 时仍**尊重**模型返回的合法 `no_task` JSON（打 `warn`）。**`staged_plan_feedback_mode`**（默认 `fail_fast`，`AGENT_STAGED_PLAN_FEEDBACK_MODE`）：`patch_planner` 时在某步 `run_agent_outer_loop` 返回 `Err` 或该步范围内存在失败 `role: tool`（信封 `ok: false` 或传统解析）时，注入反馈 user 并无工具重跑规划轮，将补丁 `steps` 与「当前步及之后」合并（`plan_artifact::merge_staged_plan_steps_after_step_failure`）后继续；次数受 **`staged_plan_patch_max_attempts`**（`AGENT_STAGED_PLAN_PATCH_MAX_ATTEMPTS`，默认 2）限制。**`staged_plan_cli_show_planner_stream`**（`AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM`，默认 true）：CLI（`out: None`）下首轮与补丁无工具规划轮的 `complete_chat_retrying` 可单独关闭 `render_to_terminal`，不在终端打印该轮模型原文（`staged_plan_notice`、分步注入与执行步输出不变）。规划轮在解析 `agent_reply_plan` 前**丢弃** API 返回的原生 `tool_calls`（部分网关在 `tool_choice: none` 下仍返回占位或误生成），再按 **`materialize_deepseek_dsml_tool_calls`** 调用 **`materialize_deepseek_dsml_tool_calls_in_message`**：若物化出非空 `tool_calls`，则 `push` 助手后走 **`per_reflect_after_assistant` → `per_execute_tools_web`**，再 **`run_agent_outer_loop`**。若 **JSON 解析失败**且未走该分支，打 `warn` 后追加 assistant 再 **`run_agent_outer_loop`**；**不再**下发 `code: staged_plan_invalid`。`plan_artifact::staged_plan_invalid_run_agent_turn_error` / `is_staged_plan_invalid_run_agent_turn_error` 仍保留供测试与 `chat_job_queue` 历史兼容分支。
- **规划器/执行器模式（阶段 1）**（`[agent] planner_executor_mode` / `AGENT_PLANNER_EXECUTOR_MODE`）：
  - `single_agent`（默认）：沿用历史单 agent 逻辑。
  - `logical_dual_agent`：同进程逻辑双 agent。规划轮仅消费去分隔线、去 `tool`、去空 assistant 的自然语言上下文，再追加规划 system 指令产出 `agent_reply_plan`；执行轮仍由既有外层循环负责工具调用与反思校验。该模式可减少工具原始输出对规划拆解的干扰，且不改变 HTTP/SSE 协议形状。

#### 配置与 P/R/E 路径对照（`run_agent_turn_common`）

**P/R/E 含义**（与 `agent_turn/mod.rs` 注释一致）：**P** = 一次 `complete_chat_retrying`（向模型要本轮输出）；**R** = `per_reflect_after_assistant` → 终答时 `per_coord::after_final_assistant`；**E** = `per_execute_tools_web`。**`staged_plan_phase_instruction`**（`AGENT_STAGED_PLAN_PHASE_INSTRUCTION`）：仅在有「无工具规划轮」时生效——非空则作为该轮追加的 **system** 规划指令，空则使用 `staged_sse::staged_plan_phase_instruction_default(staged_plan_allow_no_task)`。**`staged_plan_allow_no_task`**（`AGENT_STAGED_PLAN_ALLOW_NO_TASK`）：默认 true，为内置默认规划说明是否包含 `no_task` + 空 `steps` 的约定。**`final_plan_requirement`** / **`plan_rewrite_max_attempts`** 只作用于 **R**（模型以非 `tool_calls` 结束的轮次），**不**作用于分阶段的首轮无工具规划 assistant（该轮走 JSON 解析而非 `after_final_assistant`）。**工作流反思**：`prepare_workflow_execute` 在注入 `instruction_type == workflow_reflection_plan_next` 时置位 `plan_requirement_source`；`append_tool_result_and_reflection` 对 **`workflow_reflection_next`** 同样置位，使后续轮次终答仍与 `final_plan_requirement = workflow_reflection` 一致。

**顶层分支**（代码顺序：**`planner_executor_mode` 优先**；`logical_dual_agent` 时**无论** `staged_plan_execution` 真假都会走分阶段式「规划轮 + 分步外层循环」）：

| planner_executor_mode | staged_plan_execution | 顶层入口 | 首轮 P | 后续每步 |
|-----|-----|-----|-----|-----|
| `logical_dual_agent` | `false` 或 `true` | `run_logical_dual_agent_then_execute_steps` | 无工具 P 一轮；上下文 `build_logical_dual_planner_messages` + 规划 system（见上） | 每步：`run_agent_outer_loop` → 多轮 **P（带 tools）→ R → E** 直至该步终答 |
| `single_agent` | `true` | `run_staged_plan_then_execute_steps` | 无工具 P 一轮；上下文 `build_single_agent_planner_messages`（**保留** `role: tool`）+ 规划 system | 同上 |
| `single_agent` | `false` | `run_agent_outer_loop` | 无单独规划轮；每圈 **P（带 tools）→ R → E** | （同一循环直至本轮结束） |

**终答规划策略**（与分阶段是否开启**正交**：只要某路径进入 `run_agent_outer_loop`，每步结束时的 R 均受下表约束）：

| final_plan_requirement | plan_rewrite_max_attempts | R（无 `tool_calls` 的 assistant） |
|-----|-----|-----|
| `never` | — | 不强制 `agent_reply_plan` v1；`StopTurn` |
| `workflow_reflection`（默认） | `N`（配置 `1..=20`，默认 `2`） | 仅在工作流路径置位「需要终答规划」后校验；不合格则追加重写 user，**至多 `N` 次**（用尽 → SSE `plan_rewrite_exhausted`） |
| `always` | `N` | 每次终答均校验；同上重写上限 |

- **上下文窗口策略**（`src/agent/context_window.rs` + **`src/agent/message_pipeline.rs`**）：每次 P 步前 `prepare_messages_for_model` 先经 **`message_pipeline::apply_session_sync_pipeline`**（从 **`AgentConfig`** 派生 **`MessagePipelineConfig`**；单测可用 **`apply_session_sync_pipeline_with_config`**）做同步变换。**顺序契约**以 `message_pipeline.rs` 模块文档编号列表为准（与 `MessagePipelineStage` 一一对应）。**`/status` 计数、日志与配置告警**见上文**架构设计**「**上下文管道（观测）**」。再可选 **`maybe_summarize_with_llm`**。变换含：**`tool` 消息正文压缩**（`tool_message_max_chars`；若正文为 **`tool_result::encode_tool_message_envelope_v1`** 的 `crabmate_tool` 形状且内层 **`output`** 超长，则 **`tool_result::maybe_compress_tool_message_content`** 对其做**首尾采样**并写入 **`output_truncated`** / **`output_original_chars`** / **`output_kept_head_chars`** / **`output_kept_tail_chars`**，便于模型引用「原文规模 + 保留片段」；非信封正文仍按前缀截断）、**按条数保留**（沿用 `max_message_history`）、可选 **`context_char_budget` 按近似字符删旧消息**；若 `context_summary_trigger_chars > 0` 且非 system 总字符超阈值，则额外发起**无 tools** 的 `chat/completions` 将「中间段」压成一条 user 摘要，尾部保留 `context_summary_tail_messages` 条。Web/CLI 侧 `messages` 会随裁剪/摘要变化（工具截断不改变条数）。发往供应商的 **`ChatRequest.messages`** 由 **`message_pipeline::conversation_messages_to_vendor_body`**（或已 strip 时的 **`normalize_stripped_messages_for_vendor_body`**）统一拼装，**`llm::api::stream_chat`** 在 HTTP 前对请求体再跑一遍同一出站路径以兜底直连 `ChatRequest`。**`tool_result_envelope_v1`**（默认 true）：经 `execute_tools::emit_tool_result_sse_and_append` 写入历史的 `role: tool` 为单行 JSON，顶层键 **`crabmate_tool`**，字段含 **`v`/`name`/`summary`/`ok`/`exit_code`/`error_code`/`output`**（`summary` 与 SSE `ToolResultBody.summary` 及 `summarize_tool_call*` 同源）；经消息管道压缩后另可有 **`output_truncated`** 等元数据。`per_coord::last_workflow_validate_layer_count` 等需解析工具 JSON 的路径使用 **`tool_result::tool_message_payload_for_inner_parse`** 剥离信封后读内层。**`per_coord` 非空时**，函数结束时会调用 **`PerCoordinator::invalidate_workflow_validate_layer_cache_after_context_mutation`**，避免 `per_coord` 内缓存的 `workflow_validate` **`layer_count`** 在删旧消息后仍指向已不存在的工具结果。配置见 `config/default_config.toml` 与 `AGENT_CONTEXT_*` / `AGENT_TOOL_MESSAGE_MAX_CHARS` / **`AGENT_TOOL_RESULT_ENVELOPE_V1`**。**会话工作区变更集**：若 **`session_workspace_changelist_enabled`**（默认 true），在 **`maybe_summarize_with_llm`** 之后调用 **`workspace_changelist::sync_changelist_user_message`**（**`push`** 到 `messages` **末尾**，使含 `tool` 结果的回合里摘要紧邻「最近上下文」；下一轮 P 步开头先 **`strip`** 再跑同步管道）；**`types::messages_for_api_stripping_reasoning_skip_ui_separators`** 与长期记忆的「最后 user 查询」启发式均跳过该条。
- **系统提示词规则拼接**（`[agent] cursor_rules_enabled` / `AGENT_CURSOR_RULES_ENABLED`）：在 `config::load_config` 阶段，`system_prompt`/`system_prompt_file` 读取后可追加 `cursor_rules_dir`（默认 `.cursor/rules`）下 `*.mdc`，并可选附加工作区根 `AGENTS.md`（`cursor_rules_include_agents_md`）；按文件名排序拼接，附加段受 `cursor_rules_max_chars` 限制，超出截断并加提示。该拼接结果即后续 `messages_chat_seed` 使用的首条 `system`。
- **长期记忆与向量检索**：已实现 **会话级** SQLite 存储 + 可选 **fastembed**（CPU）余弦检索；注入条目标记 **`crabmate_long_term_memory`** 不进入供应商请求体。`/status` 暴露 `long_term_memory_*` 字段。外部 **Qdrant/pgvector** 仍为后续工作。`LongTermMemoryScopeMode` 当前仅 `conversation`；与 P0 鉴权关系见 `README.md`。
- **处理结束原因**：
  - `finish_reason != "tool_calls"`：本轮对话结束，最后一条 assistant message 即最终回复。
  - `finish_reason == "tool_calls"`：解析 tool calls，逐个执行本地工具，把工具结果作为 `role: "tool"` 的消息追加进 `messages`，然后继续下一轮请求，直到模型返回最终文本。
- **SSE 通道协作**：若本轮由 `/chat/stream` 触发，会通过 channel 向前端发送：
  - 文本 delta（assistant 内容增量）
  - **控制类 JSON**（由 **`src/sse/protocol.rs`** 序列化）：统一带版本字段 `v`（当前为 `1`），并与原有键名兼容，例如：
    - `tool_running`、`tool_result`（可选 `summary`：与 `summarize_tool_call` 同源，与 `output` 同帧；**不再**在工具执行前单独下发 `tool_call`，避免 Web 在工具未完成时先插入摘要）、`workspace_changed`
    - `error`（+ 可选 `code`）、`command_approval_request`（Web / 工作流审批）
    - `staged_plan_notice`（+ 可选 `staged_plan_notice_clear`）：分阶段规划进度；`frontend/src/api.ts` 识别为控制面并吞掉，避免当作正文 delta
    - `staged_plan_started` / `staged_plan_step_started` / `staged_plan_step_finished` / `staged_plan_finished`：分阶段规划结构化进度事件（含 `plan_id`、`step_id`、`step_index`、`status: ok|cancelled|failed` 等），用于前端按状态机消费，避免解析自然语言文案
    - 预留 `plan_required` 等扩展键
- **协议版本 `v`**：当前为 `1`；演进时递增 **`sse::protocol::SSE_PROTOCOL_VERSION`**，前端 `api.ts` 的 `sendChatStream` 已按字段形状解析（`tool_call` / `tool_result` / `plan_required` / `error.code` 等），新事件需在前后端同步扩展。

### PER 与终答 `agent_reply_plan` 强制策略

- **`agent::per_coord::PerCoordinator`**（`src/agent/per_coord.rs`）在 Web 与 CLI 共用：串联 **workflow 反思**（`workflow_reflection_controller`）与 **终答正文**是否含 `plan_artifact` 可解析的 v1 规划。终答校验用到的 **`workflow_validate_result` → `spec.layer_count`** 在历史中扫描的结果可**按会话长度缓存**（同长度连续 `after_final_assistant` 时避免重复全表扫描）；**任意 tool 结果写入**（`append_tool_result_and_reflection`）或 **`prepare_messages_for_model` 改写历史**后刷新或清空缓存。反思注入 `Value` 若 **`serde_json::to_string` 失败**（极罕见），会 **`warn`** 并追加固定占位 user JSON（`instruction_type: crabmate_reflection_serialize_failed`），避免静默空串。
- **`agent_reply_plan` v1 步骤 `id`**：须 **唯一**，且符合稳定语法（ASCII 字母或数字开头，仅含 `-` `_` `.` `/`，总长 ≤128），便于日志与跨产物对齐。**可选 `workflow_node_id`**：若出现则语法同上且在规划内唯一，且须为历史中**最近一次** `workflow_execute` 工具结果（`workflow_validate_result` 或 `workflow_execute_result`）里 **`nodes[].id`** 的子集；否则触发与缺规划同类的重写提示（与 `layer_count` 规则可同时生效）。
- **`plan_artifact::format_plan_steps_markdown` / `format_agent_reply_plan_for_display`**：对合法 v1 规划生成**简单 Markdown 有序列表**（后者另含围栏前自然语言段落）；**`format_plan_steps_markdown_for_staged_queue`** 为 CLI 终端「队列」风格摘要（步骤前 `[ ]`/`[✓]`，仅展示 `description`），每步完成后 **`send_staged_plan_notice(clear_before: true)`** 整段刷新；前端 `agentPlanDisplay` / `ChatPanel` 展示用，**不**改写 `Message.content`。解析与 **`strip_agent_reply_plan_fence_blocks_for_display`** 将围栏首行 **`json` / `markdown` / `md`**（忽略大小写，可含前导空行）视为等价语言标签；**无**语言行的裸 \`\`\` 围栏内即使以 `{` 开头也不当作规划 JSON 流式缓冲（与 `message_display` / 前端 `agentPlanDisplay` 对齐）。
- **配置项** `[agent] final_plan_requirement`（环境变量 `AGENT_FINAL_PLAN_REQUIREMENT`）→ `FinalPlanRequirementMode`：
  - **`never`**：不进入「缺规划则追加 user 重写提示」循环；反思注入仍会下发，但不置位强制标记。
  - **`workflow_reflection`（默认）**：仅当工具路径注入了 `instruction_type == workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT` 时，对随后的**最终** assistant 校验；避免与反思 JSON 的字符串散落耦合。
  - **`always`**（实验性）：每次 `finish_reason != tool_calls` 的终答均校验。只要终答缺合格 `agent_reply_plan`，就会计入重写次数并可能再调模型，**轮次与费用通常明显高于** `workflow_reflection`；适用于强约束输出形态、联调规划解析、或审计场景。低成本/闲聊场景不建议开启。
- **`[agent] plan_rewrite_max_attempts`**（`AGENT_PLAN_REWRITE_MAX_ATTEMPTS`，默认 `2`， clamp `1..=20`）：终答规划不合格时，最多追加多少次「请重写」user 消息；用尽后结束外层循环，并在 **有 SSE 通道** 时发送 `{"error":"…","code":"plan_rewrite_exhausted"}`（与 `sse::SsePayload::Error` 一致）。
- **规则化语义（相对 `workflow_validate_only`）**：当策略要求校验规划，且历史中最近一次 `workflow_execute` 的 tool 结果为 `report_type == workflow_validate_result` 时，读取 `spec.layer_count`（拓扑层数），要求 `agent_reply_plan.steps.len() >= layer_count`；否则仅做 JSON 形态校验。重写提示中会附带 `layer_count` 说明。若步骤含 **`workflow_node_id`**，另与最近一次 **`nodes[].id`** 列表做子集校验（见上条）；重写提示在可用时附带允许的节点 id 列表。
- **可观测性**：`log` 目标 `crabmate::per`（`RUST_LOG=crabmate::per=info` 或 `RUST_LOG=info`）记录 `after_final_assistant` 的 outcome、`reflection_stage_round`、`plan_rewrite_attempts` 等；`workflow_reflection_controller::WorkflowReflectionController::stage_round()` 供排错对照反思轮次。
- **CLI 消息打印路径**：`log` 目标 **`crabmate::print`**（`RUST_LOG=crabmate::print=debug`）在 `terminal_labels::write_user_message_prefix`、`terminal_cli_transcript::{print_staged_plan_notice, print_tool_result_terminal}`（工具标题 **`### 工具 · name : …`**：有详情时 **`name` 与摘要之间统一为 ` : `**；摘要与 SSE 同源的 **`summarize_tool_call`** 或单行截断的 `args`；若摘要以与工具名同义的 **`verb:`** 开头则去掉动词短语、**保留冒号与后续**（如 `create file: x` → `: x`）以免与 `name` 重复；**`read_file` / `read_dir` / `list_tree`** 在终端仅打印标题与省略说明、**不**回显工具正文，避免刷屏；完整结果仍进消息历史）、`llm::api::terminal_render_agent_markdown` 及 `runtime::cli`（交互式 CLI / **`chat` 单次**）等处记录即将打印的正文预览（截断），便于对照终端实际输出。

```mermaid
flowchart LR
  subgraph E[工具批 E]
    WF[workflow_execute]
  end
  subgraph PER[per_coord]
    PRE[prepare_workflow_execute]
    FLAG[require_plan_in_final_content]
    AFA[after_final_assistant]
  end
  WF --> PRE
  PRE -->|"policy=WorkflowReflection 且注入 plan_next"| FLAG
  AFA -->|"不合格且未超重写次数"| REW[追加 user 重写提示]
  AFA -->|"用尽重写次数"| ERR[SSE error plan_rewrite_exhausted]
  AFA -->|"JSON+层数语义 OK 或无需校验"| STOP[结束本轮外层循环]
```

- **`GET /status`** 返回 `final_plan_requirement`、`plan_rewrite_max_attempts`、`staged_plan_cli_show_planner_stream`，便于与 `reflection_default_max_rounds` 一起核对运行态；另返回 **`per_active_jobs`**（仅队列内**正在执行**的 `/chat`、`/chat/stream` 任务）：每项含 `job_id`、`awaiting_plan_rewrite_model`（已追加规划重写 user 消息、等待下一轮模型输出）、`plan_rewrite_attempts`、`require_plan_in_final_content`。与前端「会话」无稳定 id 对应；若需按会话展示，需扩展请求体/存储后再关联 `job_id` 或自建会话字段。

## 后端模块说明（`src/`）

**按文件/目录的职责一览见上文「`src/` 代码模块索引」与「`src/tools/` 子文件」**；本节按主题补充实现细节与扩展点。

### `src/lib.rs` / `src/main.rs`

- **`lib.rs`**：crate 根模块；Agent 主循环（`run_agent_turn`）、Axum Web 路由与 handler、上传清理等。**对外再导出** `run`、`load_config`、`AgentConfig`、`Message`、`Tool`、`build_tools`、`build_tools_filtered`、`build_tools_with_options`、`ToolsBuildOptions`、`dev_tag` 等，供集成测试与其它二进制复用。
- **`main.rs`**：薄入口，仅 `#[tokio::main] async fn main() { crabmate::run().await }`；**`CliExitError`** 映射约定退出码（契约见 **`tests/cli_contract.rs`**）。
- **CLI 契约测试**：集成测试 **`tests/cli_contract.rs`** 加载 **`tests/fixtures/cli/*.json`**，覆盖 **`normalize_legacy_argv`**、**`parse_args_from_argv`**（测试内串行并清理 **`AGENT_HTTP_HOST`**）及 **`classify_model_error_message`** / **`EXIT_*`** 常量。库根再导出 **`parse_args` / `parse_args_from_argv` / `normalize_legacy_argv`** 等供该套件使用。
- **运行模式**：由 `run()` 内解析 CLI。推荐使用 **子命令**：`serve`（Web）、`repl`（交互，**未写子命令时默认进入 repl**）、`chat`（单次 `--query` / `--stdin`）、`bench`（批量测评）、`config`（自检；`--dry-run` 可选且行为相同）、**`doctor`**（一页本地诊断，**不要**求 `API_KEY`）、**`save-session`**（兼容 **`export-session`**；从会话文件导出 JSON/Markdown 到 `.crabmate/exports/`，与 Web 同形，**不要**求 `API_KEY`）、**`tool-replay`**（从会话提取工具步骤 fixture / 重放，**不要**求 `API_KEY`；见 **`runtime/tool_replay.rs`**）、**`models`** / **`probe`**（`GET {api_base}/models` 列模型或探测连通性，需 `API_KEY`；输出脱敏，不打印响应体）。**`help`**：`crabmate help` → 根级 `--help`；`crabmate help serve` 等 → 子命令 `--help`。全局选项 `--config` / `--workspace` / `--no-tools` / `--log` 须写在子命令**之前**（如 `crabmate --config x serve`）。**兼容**：未写子命令时，历史平铺 flag（`--serve`、`--query`、`--benchmark`、`--dry-run` 等）会在 `parse_args` 前经 `normalize_legacy_argv` 改写为上述子命令形式；若 argv 中**任意位置**出现显式子命令名则不再插入默认 `repl`（契约见 `tests/fixtures/cli/legacy_normalize.json`）。旧脚本无需修改。**日志**：`serve` 默认 **info**；`repl` / `chat` / `bench` / `config` / `save-session`（及别名 **`export-session`**）/ **`tool-replay`** 默认 **warn**（未设 `RUST_LOG` 时）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**，并同时写 stderr 与文件。`--serve` 默认绑定 `127.0.0.1`；`0.0.0.0` 需 `serve --host` 或环境变量 `AGENT_HTTP_HOST`。非 loopback 且无 Bearer 时默认拒绝启动（见 README）。
- **Web 服务**：使用 axum 路由，核心接口包括：
  - `POST /chat`：非流式对话（请求体 `message` + 可选 `conversation_id`；可选 `temperature`（0～2）、`seed`（整数）、`seed_policy`（`omit`/`none` 表示本回合不带 seed，与 `seed` 互斥）；响应含 `conversation_id`）
  - `POST /chat/stream`：SSE 流式对话（同上；响应头 `x-conversation-id` 回传会话 ID；可选 `approval_session_id` 用于 Web 审批会话绑定）
  - `POST /chat/approval`：Web 审批回传（`deny` / `allow_once` / `allow_always`）
  - `GET /status`：状态栏数据（模型、`api_base`、**`llm_http_auth_mode`**（`bearer` / `none`）、`max_tokens`、`temperature`、**`llm_seed`**（默认 seed，未配置为 `null`）、**`tool_count` / `tool_names` / `tool_dispatch_registry`**、`reflection_default_max_rounds`、**`final_plan_requirement` / `plan_rewrite_max_attempts`**、**`max_message_history` / `tool_message_max_chars` / `context_char_budget` / `context_summary_trigger_chars`**、**`message_pipeline_trim_count_hits` / `message_pipeline_trim_char_budget_hits` / `message_pipeline_tool_compress_hits` / `message_pipeline_orphan_tool_drops`**（自进程启动以来同步管道**实际**触发次数，累计非会话级）、**`chat_queue_*` / `parallel_readonly_tools_max` / `chat_queue_recent_jobs` / `per_active_jobs`**、`conversation_store_entries`、**`long_term_memory_enabled` / `long_term_memory_vector_backend` / `long_term_memory_store_ready` / `long_term_memory_index_errors`**）
  - `GET /health`：健康检查（API_KEY/静态目录/工作区可写/依赖命令）；实现见 `health.rs`。
  - `GET|POST /workspace` + `GET|POST|DELETE /workspace/file`：工作区浏览与读写文件（`GET /workspace/file` 仅读取不超过 1 MiB；正文解码与 `read_file` 一致，可选查询参数 **`encoding`**，默认 UTF-8 严格，非法序列返回错误而非有损替换）。`POST /workspace` 对非空路径执行目录存在性、`workspace_allowed_roots` 白名单与敏感系统目录黑名单校验，避免把运行时工作区切到 `/proc`、`/sys`、`/dev`、`/etc`、`/usr` 等区域。
  - `GET|POST /tasks`：任务清单读写
  - `POST /upload` + `GET /uploads/...`：上传与静态访问
- **状态与工作区选择**：`AppState` 内维护 `workspace_override`，由前端调用 `/workspace` POST 来设置，影响 Agent 的工具执行工作目录与文件 API 根目录。
- **Web 对话队列**：`src/chat_job_queue.rs` 的 `ChatJobQueue` 对 `/chat`、`/chat/stream` 做**有界**排队与**并发上限**（`chat_queue_max_concurrent` / `chat_queue_max_pending`）；满则 **503** + `QUEUE_FULL`。流式任务在 **`mpsc::Receiver` drop** 时经 **`Sender::closed()`** 置位 **`AtomicBool` cancel**（打 **info**）；`llm::api::stream_chat` 在 **`out` 投递失败**且提供 **`cancel`** 时由 **`sse::send_string_logged_cooperative_cancel`** 协作置位，尽快结束上游 SSE 消费。任务标记为取消且 **SSE 仍可投递** 时补发控制面 **`error` + `code: STREAM_CANCELLED`**（见 **`docs/SSE_PROTOCOL.md`**）。`/status` 暴露 `chat_queue_completed_ok` / `chat_queue_completed_cancelled` / `chat_queue_completed_err` 与 `chat_queue_recent_jobs[*].cancelled`。单进程内协调，多副本需外部代理（见 `TODOLIST`）。

### `src/llm/mod.rs`

- **与大模型交互的封装层**（在 `backend` / `api` 之上）：`tool_chat_request` / `no_tools_chat_request` 从 `AgentConfig` + `messages`（+ `tools`）构造 `ChatRequest`（含 `temperature`、`seed` 可选字段与 `tool_choice`）。`tool_chat_request` 经 **`agent::message_pipeline::conversation_messages_to_vendor_body(..., cfg.llm_fold_system_into_user)`** 生成 `messages`（strip UI/长期记忆与 reasoning + `types::normalize_messages_for_openai_compatible_request`，可选再 **`types::fold_system_messages_into_following_user`**）。**`no_tools_chat_request_from_messages`**：接受已 strip 的 `Vec<Message>`，经 **`normalize_stripped_messages_for_vendor_body`**。`stream_chat` 在发送 HTTP 前对 `req.messages` 再执行 **`conversation_messages_to_vendor_body`**（含同一折叠开关），防止绕过构造器。**`complete_chat_retrying`** 对 **`&dyn ChatCompletionsBackend`** 调用 `stream_chat`，并做 **指数退避重试**（`api_max_retries` / `api_retry_delay_secs`）。**`llm_fold_system_into_user`**：代码与**嵌入 TOML** 默认 **`false`**（与默认 **`model = deepseek-chat`** 一致）；接 **MiniMax** 等常拒独立 **`system`** 条的网关时请设 **`true`**（见 **`docs/CONFIGURATION.md`**「MiniMax」）。
- **Agent 主循环**（`agent::agent_turn::per_plan_call_model_retrying`）与 **`context_window`** 经同一后端引用调用本模块，避免在 P 步与摘要路径重复拼装重试逻辑。
- HTTP 路径片段见 `types::OPENAI_CHAT_COMPLETIONS_REL_PATH`（`api` / 文档共用）；模型列表见 `types::OPENAI_MODELS_REL_PATH`（**`openai_models`** / CLI `models`、`probe`）。

### `src/llm/openai_models.rs`

- **`fetch_models_report`**：`GET {api_base}/models`；**`llm_http_auth_mode=bearer`** 时带 **`Authorization: Bearer {API_KEY}`**，**`none`** 时不发送该头。解析 OpenAI 形 `data[].id`；**不**将响应体写入日志或终端全文；展示用 URL 若含 query 则折叠为 `?…`。

### `src/llm/backend.rs`

- **`ChatCompletionsBackend`**：`async_trait` trait，与 `api::stream_chat` 同签名；默认实现 **`OpenAiCompatBackend`**（进程内单例 **`OPENAI_COMPAT_BACKEND`**），**`default_chat_completions_backend()`** 返回其 `&dyn` 引用。
- 库根再导出 **`ChatCompletionsBackend`**、**`OpenAiCompatBackend`**、**`OPENAI_COMPAT_BACKEND`**、**`default_chat_completions_backend`**，供嵌入 `run_agent_turn` 时注入自定义后端。

### `src/http_client.rs`

- **`build_shared_api_client`**：`run()` 内构造**唯一**异步 `reqwest::Client` 写入 `AppState`，供所有 `chat/completions` 与工具内嵌 HTTP 调用以外的模型流量复用。
- **连接优化**（非 WebSocket）：`connect_timeout` 与整请求 `timeout` 分离；`pool_max_idle_per_host`、`pool_idle_timeout`、`tcp_keepalive` 便于 **HTTP Keep-Alive / 连接池** 在多轮对话中复用 TLS（OpenAI 兼容 API 为 HTTP+SSE，无「单条模型 WebSocket」协议）。

### `src/llm/api.rs`

- **单次 HTTP 传输**：`POST {api_base}/chat/completions`，`stream: true` 时对响应进行 `data: ...` 行拆解，聚合 assistant content 与 tool_calls（按 index 累积 arguments）。流结束时若缓冲区内仍有**未以换行结尾**的最后一帧，会在关闭读循环后补解析一次，避免尾部 delta 丢失（此前仅按 `\n` 切行时易丢末包）。**MiniMax `reasoning_split`**：当 **`AgentConfig::llm_reasoning_split`** 为真时，`ChatRequest` 带 **`reasoning_split: true`**；SSE 中除 **`delta.reasoning_content`** 外，若存在 **`delta.reasoning_details`**（JSON 数组、元素常含 **`text`**），按「相对上一块的累积全文」做增量追加到 **`reasoning_acc`**（与 **`reasoning_content`** 共用下游展示）；非流式响应在解析后对 assistant **`Message`** 调用 **`merge_reasoning_details_into_reasoning_content`**。
- **终端输出（CLI）**：`render_to_terminal` 为 true 时，SSE **不在**收包过程中向 stdout 写正文（避免半段 Markdown）；**整段到达后**与 **`--no-stream`** 一致：先输出加粗着色的 **`Agent: `** 前缀（`runtime::terminal_labels::write_agent_message_prefix`，洋红），正文经 **`message_display::assistant_markdown_source_for_display`** 再 **`markdown_to_ansi`**。**交互式 CLI**（TTY）左提示由 **`repl_reedline`** 渲染，字节与 **`write_user_message_prefix`（`我: ▸ `）** / **`write_repl_bash_prompt_prefix`（`bash#: ▸ `）** 一致；**空行**按 **`$`/`＄`** 即切换 shell 模式（仍兼容 **`$` + Enter**）。当 **`out: None`**（`run_agent_turn` 的 CLI 路径）时，另由 **`runtime::terminal_cli_transcript`** 打印 **`staged_plan_notice` 等价文本**（`send_staged_plan_notice` 内；经 **`user_message_for_chat_display`**）、**分步注入 user**（`agent_turn` 在 `echo_terminal_staged` 时另调 **`print_staged_plan_notice`**）以及**各工具返回**（与 **`message_display::tool_content_for_display_full`** 一致，超长按 `command_max_output_len` 截断）；行颜色与 **`cli_repl_ui::CLI_REPL_HELP_*_FG`** 同源并尊重 **`NO_COLOR`**／非 TTY。**不得**用光标上移 + `Clear(FromCursorDown)` 整屏重绘，以免与 **run_command** 等子进程输出错位。

### `src/sse/protocol.rs`

- **SSE 控制帧**：`SseMessage { v, payload }` + `SsePayload`（`serde` untagged），`encode_message` 生成单行 JSON；Web **`agent::agent_turn`**、**`agent::workflow`** 审批、流式错误等均经此发出，避免手写 JSON 拼写错误。
- **对外文档**：版本号、`error`/`code` 与 `tool_result.error_code` 枚举、控制面变体表、与 `api.ts` 的对齐清单见 **`docs/SSE_PROTOCOL.md`**（修改协议时须同步该文件与前端 `SseControlPayload` / `tryDispatchSseControlPayload`）。

### `src/sse/line.rs`

- **消费侧分类**：将单条 SSE `data:` 字符串分为工具状态、`tool_call`（含 `name/summary`）、审批请求、`tool_result`（含 `name/summary/ok/exit_code/error_code`）、工作区刷新、流错误、忽略或正文（`Plain`）；与 **`protocol`** 反序列化及若干历史裸 JSON 键名兼容。与 `frontend/src/api.ts` 的 `tryDispatchSseControlPayload` 语义对齐；当前 crate 根**不**再导出本模块符号。

### `src/types.rs`

- **统一数据结构**：请求/响应、message、tool schema、stream chunk 等类型。
- **关键点**：tool calling 依赖 `Tool`（function 名、描述、JSON schema）与 `Message.tool_calls` / `role: "tool"` 消息回填。**`ChatRequest::reasoning_split`**：可选布尔，序列化为供应商字段 **`reasoning_split`**。**`Message::reasoning_details`**：部分供应商（如 MiniMax）在 **`reasoning_split`** 下返回的结构化思维链片段；出站发往供应商前与 **`reasoning_content`** 一并剥离（**`message_clone_stripping_reasoning_for_api`**），成功解析后合并进 **`reasoning_content`**（**`merge_reasoning_details_into_reasoning_content`**）。
- **发往供应商前的消息管线**：`messages_stripping_reasoning_for_api_request`（全量 strip）；`messages_for_api_stripping_reasoning_skip_ui_separators`（strip + 跳过 `crabmate_ui_sep`）；`normalize_messages_for_openai_compatible_request`（合并相邻 assistant 等，仅用于 `ChatRequest.messages` 拼装）。

### `src/tools/file/`（节选）

- 实现由 **`file/mod.rs`** 聚合子模块，对外仍通过 **`tools::file::read_file`** 等路径调用（`tools/mod.rs` 中 `mod file` 不变）。
- 除 `read_dir` 外，`glob_files`（`glob` crate 模式 + 工作区内递归）与 `list_tree`（先序目录树）均带 **深度/条数上限**，并对 `canonicalize` 结果做工作区根校验，避免符号链接逃逸。
- **`resolve_for_read`**、**`canonical_workspace_root`** 为 `pub(crate)`（由 **`file/mod.rs`** 再导出），供 `markdown_links`、`structured_data`、`table_text` 等只读工具复用（`resolve_for_read` 要求目标已存在）。

### `src/tools/mod.rs`（工具注册与分发的“表驱动”中心）

- **工具注册**：通过 `ToolSpec { name, description, category, parameters, runner }` 静态表定义每个工具。
- **顶层分类 `ToolCategory`**（供 `build_tools_filtered` 与文档）：**`Basic`（基础工具）**——时间/计算/天气、`web_search`、`http_fetch` / `http_request`、日程提醒等；**`Development`（开发工具）**——工作区文件、Git、**Rust**（Cargo/RA）、**前端**（npm）、**Python**（ruff、pytest、mypy、`uv sync`/`uv run`、pip/uv 可编辑安装）、**pre-commit**、Lint 聚合、补丁、符号搜索、工作流等。
- **Development 子域标签**（`src/tools/dev_tag.rs`）：按 **工具名** 映射到字符串标签（可多枚），用于在不增加 `ToolCategory` 枚举的前提下按语言栈/场景裁剪发给模型的工具列表。约定标签名：`general`（工作区/壳/编排/元数据等跨语言）、`vcs`（Git）、`rust`（Cargo/RA 等）、`frontend`（npm 脚本类）、`python`（ruff/pytest/mypy/uv/pip 等）、`quality`（Lint/审计/CI 聚合等与质量相关的工具，常与 `rust`/`frontend`/`python` 重叠）。映射函数为 `dev_tag::tags_for_tool_name`；**新增 `Development` 工具时须在该 `match` 中补全对应分支**（未列出的名称会回落到仅 `general`，便于不崩，但应显式维护）。
- **构建与过滤**：
  - `build_tools()`：等价于 `build_tools_with_options(ToolsBuildOptions::default())`，不按分类与标签过滤。
  - `build_tools_filtered(categories)`：仅按 `ToolCategory` 过滤；`dev_tags` 为不限制。
  - `build_tools_with_options(ToolsBuildOptions { categories, dev_tags })`：`categories` 为 `None` 或空切片时不按分类过滤；`dev_tags` 为 `None` 或空切片时不按标签过滤；否则 **仅对 `Development` 工具** 要求 `tags_for_tool_name(name)` 与 `dev_tags` **有交集**，`Basic` 仍只受 `categories` 约束。
  - `dev_tag::suggest_dev_tags_for_workspace(root)`：根据是否存在 `Cargo.toml`、`frontend/package.json` 或根目录 `package.json`、`pyproject.toml` / `setup.py` / `setup.cfg` / `requirements.txt` 等，返回建议标签列表（始终含 `general` 与 `vcs`）。
- **对外接口**（库根 `lib.rs` 再导出 `build_tools_filtered`、`build_tools_with_options`、`ToolsBuildOptions`、`dev_tag`）：
  - `tool_context_for(cfg, allowed_commands, working_dir)`：从 `AgentConfig` 构造 `ToolContext`（含 `web_search_*` 等）。
  - `run_tool(name, args_json, &ToolContext)`：按 name 分发执行。
  - `summarize_tool_call(...)`：生成 Web/SSE/TUI/CLI 共用的英文「工具调用摘要」。
  - `is_compile_command_success(...)`：识别编译命令成功以触发工作区刷新。
- **扩展新工具的建议步骤**：
  - 新增 `src/tools/<tool>.rs` 实现 runner
  - 在 `src/tools/mod.rs`：
    - `mod <tool>;`
    - 增加参数 schema builder（`params_xxx`）
    - 增加 runner（`runner_xxx`）
    - 在 `tool_specs()` 中注册 `ToolSpec`
  - 若为 **`Development`**：在 **`src/tools/dev_tag.rs`** 的 `tags_for_tool_name` 中增加该 `name` 的标签映射

### 典型工具实现说明（`src/tools/`）

- **`time.rs`**：本地时间与月历格式化（`mode=time|calendar|both`）。
- **`calc.rs`**：通过 `bc -l` 计算表达式（避免 shell 注入：通常用 stdin 传参、限制输出）。
- **`unit_convert.rs`**：`convert_units`，基于 **`uom`**（`si` + `f64`）做长度/质量/温度/信息量/时间/面积/压强/速度换算；不执行外部程序。
- **`weather.rs`**：调用 Open‑Meteo（无需 key），带超时控制。
- **`web_search.rs`**：`reqwest::blocking` + `serde` 调用 Brave Web Search 或 Tavily；Key 与 `web_search_provider` 来自 `AgentConfig`。Web 路径在 `tool_registry` 中登记为 `WebSearchSpawnTimeout`（`spawn_blocking` + 超时）。
- **`http_fetch.rs`**：`http_fetch`（阻塞 GET/HEAD）与 `http_request`（阻塞 POST/PUT/PATCH/DELETE + 可选 JSON body）；共用 `redirect::Policy` 记录重定向跳数与响应截断。二者在 **`tool_registry` 异步路径**（Web 流式 + 审批会话、CLI）下未匹配 `http_fetch_allowed_prefixes` 时均可走审批（`http_request` 白名单键为 **`http_request:<METHOD>:<URL>`**）；**`run_tool` / workflow 节点**仍仅白名单前缀。
- **`command.rs`**：命令白名单 + 超时 + 输出截断；配合 `allowed_commands` 与工作区路径限制。
- **`package_query.rs`**：Linux 包查询（`dpkg-query` / `rpm`）只读封装，统一返回安装状态、版本与来源字段，不执行安装/卸载。
- **`exec.rs`**：仅允许在工作区内运行相对路径可执行文件（禁止绝对路径与 `..` 越界）。
- **`file/`**：工作区内创建/覆盖/复制/移动文件；`resolve_for_read` / `resolve_for_write` 与祖先 symlink 校验是安全边界的关键（见 **`file/path.rs`**）；`copy_file` / `move_file` 仅针对常规文件，`overwrite` 控制目标已存在时的覆盖策略；`hash_file` 仅对常规文件流式哈希（`sha256` / `sha512` / `blake3`），可选 `max_bytes` 前缀模式。
- **`schedule.rs`**：提醒/日程；以 JSON 持久化到 `<working_dir>/.crabmate/reminders.json` 与 `events.json`。
- **`spell_astgrep_tools.rs`**：`typos_check` / `codespell_check` 仅传相对路径、不写回；`typos_check` 支持 `config_path`（项目词典通常通过 typos 配置维护），`codespell_check` 支持 `dictionary_paths`（`-I`）与 `ignore_words_list`（`-L`）；`ast_grep_run` 调用 `ast-grep run` 做结构化搜索；`ast_grep_rewrite` 在此基础上增加 `--rewrite`，默认 dry-run，`dry_run=false` 时需 `confirm=true` 才执行 `--update-all` 写盘。
- **`grep.rs` / `format.rs` / `lint.rs`**：面向开发工作流的辅助能力（搜索/格式化/静态检查聚合）；`format` 对 `.py` 使用 `ruff format`，对 `.c` / `.h` / `.cpp` / `.cc` / `.cxx` / `.hpp` / `.hh` 使用 `clang-format`（检查模式为 `--dry-run --Werror`）；`run_lints` 可选聚合 `ruff check`（`run_python_ruff`）。`run_command` 默认 **`allowed_commands`**（见 `config/tools.toml`）另含常用 **coreutils 类**（如 `stat`、`readlink`、`find`、`mkdir`、`grep`/`egrep`/`fgrep`、`sort`/`uniq`/`cut`/`tr`、`diff`/`cmp`、`which`/`whereis`…）、**系统与进程信息**（`ps`、`free`、`uptime`、`hostname`、`lscpu`、`nproc`、`lsblk`…）、**二进制查看**（`xxd`、`hexdump`、`od`、`ldd`）、**压缩流读出**（`zcat`、`bzcat`、`xzcat`）、**jq**，以及 **`git` / `cargo` / `rustc`** 与 **编译链**（`cmake`、`ctest`、`ninja`、`gcc`、`clang`、`make`、Autotools…）及 **GNU Binutils**（`objdump`、`nm`、`readelf`、`strings`、`size`、`ar`）。嵌入默认**仅**提供 **`allowed_commands`**（见 **`config/tools.toml`**）；生产环境若需收窄，请直接修改覆盖配置中的 **`allowed_commands`** 或使用 **`AGENT_ALLOWED_COMMANDS`**。`cmake`、`ctest`、`c++filt` 与 `clang-format` 等可选依赖会在 **`GET /health`** 中体现为 `dep_cmake` / `dep_ctest` / `dep_cxxfilt` / `dep_clang_format`；**Binutils** 对应 `dep_objdump` / `dep_nm` / `dep_readelf` / `dep_strings_binutils` / `dep_size` / `dep_ar`；可选 CLI **typos** / **codespell** / **ast-grep** 对应 `dep_typos` / `dep_codespell` / `dep_ast_grep`（缺失为 degraded，不阻止启动）。**`run_command` 参数**仍禁止 `..` 与以 `/` 开头的实参，CMake 场景宜使用相对 `-S`/`-B` 与 `--build`。Autotools 与 `git`/`cargo` 会执行项目内逻辑，仅宜在**可信工作区**使用。
- **`python_tools.rs` / `precommit_tools.rs`**：见上表；`quality_workspace` / `ci_pipeline_local` 可选步骤含 ruff/pytest/mypy；`pre_commit_run` 依赖仓库根 `.pre-commit-config.yaml`（或 `.yml`）。
- **`source_analysis_tools.rs`**：源码分析工具（均为只读）；`shellcheck_check` 递归查找 `.sh`/`.bash` 文件并运行 ShellCheck；`cppcheck_analyze` 对 C/C++ 代码运行 cppcheck；`semgrep_scan` 运行 Semgrep SAST 安全扫描；`hadolint_check` 对 Dockerfile 运行 Hadolint lint；`bandit_scan` 对 Python 代码运行 Bandit 安全分析；`lizard_complexity` 运行 lizard 圈复杂度分析。均需本机安装对应 CLI，缺失时返回说明性错误。对应 health 检查项：`dep_shellcheck` / `dep_cppcheck` / `dep_semgrep` / `dep_hadolint` / `dep_bandit` / `dep_lizard`。

### `src/web/*` 与 `src/runtime/*`

- **`web`**：承载 Web 侧的“工作区/任务”等 axum handler（与前端面板直接对应）。
- **`runtime`**：CLI 运行时逻辑，负责交互式 CLI、**`chat` 单次**与调用 `run_agent_turn`。
  - **`runtime/workspace_session`**：`.crabmate/tui_session.json` 加载；**`initial_workspace_messages`** 供 CLI，且**仅当** `[agent] repl_initial_workspace_messages_enabled` 为 true（默认 false；**`AGENT_REPL_INITIAL_WORKSPACE_MESSAGES_ENABLED`**）时在 **`run_repl`** 中经 **`std::thread::spawn`** 后台构建，主循环 **`try_merge_background_initial_workspace`** 合并；否则启动始终为 **`repl_bootstrap_messages_fast`**（仅一条 `system`）。**仅当** `tui_load_session_on_start` 为 true 时从磁盘恢复，并按 `tui_session_max_messages` / `AGENT_TUI_SESSION_MAX_MESSAGES` 截断。`save_workspace_session` / `export_*` 保留在代码中供后续全屏终端 UI 再接。Web 与 CLI 在**会话持久、审批、导出**上的产品差异见 **`docs/CLI.md`**「CLI 与 Web 能力对照」。
  - **`runtime/benchmark/`**：批量无人值守测评子系统（SWE-bench / GAIA / HumanEval 等）。由 CLI `--benchmark` + `--batch` 触发，在 `lib.rs::run()` 中分派。
  - **`runtime/cli_doctor`**：`doctor` / `models` / `probe` 子命令实现；`doctor` 复用 `tools::capture_trimmed` 与 **`canonical_workspace_root`**（`tools/mod` `pub(crate)` 再导出）。
  - **`runtime/cli_mcp`**：**`mcp list`** 只读输出进程内 MCP 缓存（`mcp::cached_mcp_status`）；可选 **`--probe`** 调用 `try_open_session_and_tools` 刷新缓存。交互式 CLI **`/mcp`**、**`/mcp list`**、**`/mcp probe`** 走同一实现（`run_mcp_list`，`repl_context=true` 时提示语指向 **`/mcp probe`**）。
  - **`runtime/config_reload`**：**`reload_shared_agent_config`**：`load_config` → **`apply_hot_reload_config_subset`** → **`mcp::clear_mcp_process_cache`**；供交互式 CLI **`/config reload`** 与 **`POST /config/reload`** 共用。
  - **`runtime/tool_replay`**：从 **`ChatSessionFile`** 消息序列提取 `assistant.tool_calls` 与对应 `tool` 消息，写出 **`ToolReplayFile`** fixture；**`tool-replay run`** 按步调用 **`tools::run_tool`**（与 Agent 路径相同，**无** LLM、**无** CLI 审批交互）。**`--compare-recorded`** 与 `recorded_output` 全等比较失败时退出码 **`EXIT_TOOL_REPLAY_MISMATCH`（6）**。

## 前端模块说明（`frontend/src/`）

### `frontend/src/api.ts`

- **统一请求封装**：超时、重试、错误分类（`ApiError`）、GET 去重与轻量缓存（SWR）。
- **SSE 协议版本**：导出常量 **`SSE_PROTOCOL_VERSION`**，须与 **`sse::protocol::SSE_PROTOCOL_VERSION`** 及 **`docs/SSE_PROTOCOL.md`** 一致。
- **流式聊天**：`sendChatStream` 消费 `/chat/stream` 的 SSE，把：
  - 请求体中的可选 `conversation_id` 传给后端；若首轮未传，读取响应头 `x-conversation-id` 并缓存到面板状态
  - 纯文本 `data:` 当作 delta
  - JSON `data:` 识别 `tool_running`/`tool_call`（兼容旧服务端）/`tool_result`（含可选 `summary`）/`workspace_changed`/`command_approval_request` 并分发回调；审批决策通过 `submitChatApproval` 发到 `POST /chat/approval`

### `frontend/src/components/ChatPanel.tsx`

- **聊天主面板**：维护消息列表、流式渲染（尽量只更新最后一条 assistant），以及工具输出的“系统消息卡片”（可折叠/复制）。
- **附件**：图片/音频/视频本地压缩/转 DataURL（当前实现以 DataURL 形式随消息发送/展示；上传 API 也已在 `api.ts` 提供，用于走服务端 `/upload`）。
- **会话导出**：把当前对话导出为 JSON。

### `frontend/src/components/WorkspacePanel.tsx`

- **工作区浏览/编辑**：调用 `/workspace` 与 `/workspace/file` 做目录浏览、文件读写、删除与下载。`frontend/src/api.ts` 会在请求时自动附带 `localStorage["crabmate-api-bearer-token"]`（若存在）作为 `Authorization: Bearer <token>`，用于 Web API 鉴权。
- **工作区设置**：把用户选择的目录同步到后端（`POST /workspace`），并本地持久化到 `localStorage`。
- **目录内搜索**：调用 `/workspace/search`，并可“一键把结果发到聊天”。

### `frontend/src/components/TasksPanel.tsx`

- **任务清单**：读写 `/tasks`（后端按**当前生效工作区路径**保存在 **`AppState.web_tasks_by_workspace`**，**进程内存**；服务重启后丢失；**不**写工作区 `tasks.json`）。
- **从描述生成**：用一次独立 `/chat` 请求让模型输出严格 JSON，然后 `POST /tasks`。

### `frontend/src/components/StatusBar.tsx`

- **状态轮询**：轮询 `/status`，页面不可见时暂停；失败指数退避。
- **忙碌状态**：结合 Chat 面板的 `busy` 与 `toolBusy` 展示“模型生成中…”/“工具运行中…”。

## 数据与文件持久化约定

- **工作区根目录（后端当前生效目录）**：
  - Web 侧栏**任务清单**：仅存 **`serve` 进程内存**（`/tasks`），**不**在此目录落盘。
  - `.crabmate/`：提醒与日程（`reminders.json` / `events.json`）
- **前端本地存储（`localStorage`）**：
  - 工作区路径选择（`agent-demo-workspace-dir`）
  - 聊天输入框高度（`agent-demo-input-height`）

## 常见扩展点与注意事项

- **新增/调整工具**：优先在 `src/tools/mod.rs` 的表驱动体系里注册，保证 schema/runner/分类一致。
- **安全边界**：
  - `run_command` 必须受白名单控制，避免破坏性命令；**仅**用于白名单内的系统命令（编译器、make、ls 等）。运行工作区内可执行文件（如 `./main`、编译产物）须用 **`run_executable`**，不要用 `run_command`。
  - 文件读写与 `run_executable` 必须做路径归一化与越界限制。
  - Web 模式下的工作区设置会影响“工具执行目录”，需要明确这一点避免误操作。
  - **密钥与日志**：勿将真实 API key、token、`.env` 内容写入代码、示例配置、commit message 或日志；日志与错误回显须脱敏。Cursor 规则见 **`.cursor/rules/secrets-and-logging.mdc`**。
  - 已知 HTTP 鉴权、监听地址、`workspace_set` 等安全与协议债见 [`docs/TODOLIST.md`](TODOLIST.md)。
- **SSE 协议演进**：后端以 **`sse::protocol::SseMessage` / `SsePayload`**（及 `sse/mod.rs` 再导出）为单一事实来源；`v` 递增时前端可按版本分支。Rust 侧行分类见 **`sse/line.rs`**；浏览器侧在 **`frontend/src/sse_control_dispatch.ts`**（`tryDispatchSseControlPayload`，由 `api.ts` 的 `sendChatStream` 调用）。**人读契约与错误码表**：**`docs/SSE_PROTOCOL.md`**。**控制面分类契约测试**：`fixtures/sse_control_golden.jsonl` + `cargo test golden_sse_control` + `cd frontend && npm run verify-sse-contract`。

