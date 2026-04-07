**语言 / Languages:** 中文（本页）· [English](en/CLI.md)

# 命令行与子命令

查看帮助：`crabmate --help`、`crabmate help`、`crabmate help <子命令>`（与 `--help` 等价）。根级与 **`chat --help`** 文末含 **`docs/CLI_CONTRACT.md`**、**`docs/SSE_PROTOCOL.md`** 交叉引用。**全局选项**写在子命令**之前**：`--config`、`--workspace`、`--agent-role`、`--no-tools`、`--log`。

**脚本契约**（退出码、`chat --output json` 行内 JSON 的 `type`/`v` 等）：[`CLI_CONTRACT.md`](CLI_CONTRACT.md)。

## 手册页（troff / `man`）

- **源码树**：预生成的 **`man/crabmate.1`**（troff），与当前 `clap` 定义一致；**Debian `.deb`** 会安装到 **`/usr/share/man/man1/crabmate.1`**（见根目录 `Cargo.toml` 的 `[package.metadata.deb] assets`）。
- **再生成**（增删子命令或全局选项后）：`cargo run --bin crabmate-gen-man`，然后提交更新后的 `man/crabmate.1`。
- **`cargo install`**：默认**不会**把 man 装到 `MANPATH`；可将 `man/crabmate.1` 拷到本机 `.../share/man/man1/` 后执行 `mandb`（视发行版而定），或优先使用 **`cargo deb`** / 发行版打包。

## 子命令一览

| 子命令 | 说明 |
|--------|------|
| `serve [PORT]` | Web UI + HTTP API，默认 **8080**；**`bearer` 时可在未设 `API_KEY` 的情况下启动**，对话前须在侧栏「设置」填写密钥（`client_llm`）。 |
| `repl` | 交互式对话；**不写子命令时默认进入 repl**。**`bearer` 且无环境变量 `API_KEY` 时**可用 **`/api-key set <密钥>`** 后再发消息。 |
| `chat` | 单次/脚本对话：`--query` / `--stdin` / `--user-prompt-file`、`--system-prompt-file`、`--messages-json-file`、`--message-file`（JSONL）、`--yes` / `--approve-commands`、`--output json`、`--no-stream`。**`bearer` 且无 `API_KEY` 时**首轮会失败，须先 export 或使用 `repl`/`serve` 的上述方式。 |
| `bench` | 批量测评：`--benchmark`、`--batch` 等。 |
| `config` | 配置与 `API_KEY` 状态自检；`--dry-run` 可选。 |
| `doctor` | 本地诊断（**不需要** `API_KEY`）。 |
| `models` | `GET …/models`（需 `API_KEY`）。 |
| `probe` | 探测 models 端点（需 `API_KEY`）。 |
| `save-session` | 从会话文件导出 JSON/Markdown 到工作区 **`.crabmate/exports/`**（与 Web 导出同形；**不要**求 `API_KEY`）。`--format json|markdown|both`（默认 `both`），`--session-file` 可选。兼容别名 **`export-session`**。 |
| `tool-replay` | 从会话 JSON 提取**工具调用时间线**为 fixture，或按 fixture **重放工具**（与对话相同走 `run_tool`，**不调用大模型**；**不要**求 `API_KEY`）。见下文「工具重放 fixture」。 |
| `mcp list` | 只读列出**本进程**内与当前 `mcp_enabled` + `mcp_command` 指纹一致的已缓存 MCP stdio 会话及合并后的 OpenAI 工具名（**不要**求 `API_KEY`）。若尚未在本进程跑过对话，可先 **`mcp list --probe`** 尝试连接一次（会启动配置中的 MCP 子进程，与正常对话路径相同）。 |

## 日志级别

未设置 `RUST_LOG` 时：`serve` 默认 **info**；`repl` / `chat` / `bench` / `config` / `mcp` / `save-session`（及别名 `export-session`）/ `tool-replay` 默认 **warn**。可用 `RUST_LOG` 或 `--log <FILE>`。

## 消息管道调试日志

`RUST_LOG=crabmate=debug` 时每次调用模型前打印 **`message_pipeline session_sync`** 汇总；更细：`RUST_LOG=crabmate::message_pipeline=trace`。说明与 **`GET /status`** 计数见 **`docs/DEVELOPMENT.md`**「**架构设计** → **上下文管道（观测）**」；实现见 `src/agent/message_pipeline.rs`。

## 兼容旧用法

未写子命令时仍可用 `--serve`、`--query`、`--benchmark`、`--dry-run` 等，内部映射为对应子命令。若参数中**任意位置**出现显式子命令名（如 `serve` / `doctor` / `save-session` / `export-session` / `tool-replay`），则整段 argv 不再插入默认 `repl`（与 `tests/fixtures/cli/legacy_normalize.json` 契约一致）。

## 常用选项（兼容写法）

| 选项 | 说明 |
|------|------|
| `--config <path>` | 指定配置文件（建议写在子命令前） |
| `--serve [port]` | 等价于 `serve` |
| `--host <ADDR>` | 随 `serve` |
| `--query` / `--stdin` | 等价于 `chat` |
| `--workspace <path>` | 覆盖初始工作区 |
| `--agent-role <id>` | 新建 `repl` / `chat` 会话首条 `system` 用命名角色（须与配置一致；与 `chat --system-prompt-file` 互斥） |
| `--output` | 随 `chat`：`plain` 或 `json` |
| `--no-tools` | 禁用工具 |
| `--no-web` / `--cli-only` | 仅 API |
| `--dry-run` | 映射为 `config` |
| `--no-stream` | 随 `repl` / `chat` |
| `--log <FILE>` | 日志文件 + stderr 镜像 |

## Benchmark（`bench`）

| 选项 | 说明 |
|------|------|
| `--benchmark <TYPE>` | `swe_bench`、`gaia`、`human_eval`、`generic` |
| `--batch <FILE>` | 输入 JSONL |
| `--batch-output <FILE>` | 默认 `benchmark_results.jsonl` |
| `--task-timeout <SECS>` | `0` 不限制 |
| `--max-tool-rounds <N>` | `0` 不限制 |
| `--resume` | 跳过已有 `instance_id` |
| `--bench-system-prompt <FILE>` | 覆盖 system |

## 示例

```bash
cargo run                                    # 默认 repl
cargo run -- --config /path/to/my.toml serve
RUST_LOG=debug cargo run -- --log /tmp/crabmate.log repl
cargo run -- serve
cargo run -- serve 3000
cargo run -- serve --port 3000               # 与上一行等价
cargo run -- --workspace /path/to/project serve 8080
cargo run -- serve --host 0.0.0.0            # 注意安全与鉴权
cargo run -- chat --query "北京今天天气怎么样"
cargo run -- chat --output json --query "…"
echo "1+1?" | cargo run -- chat --stdin
cargo run -- --no-tools serve
cargo run -- bench --benchmark swe_bench --batch tasks.jsonl --batch-output results.jsonl --task-timeout 600
cargo run -- config
cargo run -- save-session
cargo run -- save-session --format json --workspace /path/to/proj
```

## `save-session`

默认读取 **`<workspace>/.crabmate/tui_session.json`**（`--workspace` 与全局 `--config` 写在子命令前），在 **`<workspace>/.crabmate/exports/`** 下生成带时间戳的 **`chat_export_*.json`** / **`chat_export_*.md`**（与 Web 前端导出约定一致，见 `runtime/chat_export.rs` 与 `frontend-leptos/src/lib.rs`）。每行 stdout 为写出文件的绝对路径，便于脚本捕获。

## `tool-replay`（工具时间线 fixture）

用于复现某次对话中的**工具调用顺序与参数**，或做**回归对比**（与录制时的 `tool` 消息输出是否一致）。

- **`export`**：从 **`ChatSessionFile`**（与 `save-session` / Web 导出同形）扫描 `assistant.tool_calls` 及紧随其后的 `role=tool` 消息，写出 **`tool_replay_YYYYMMDD_HHMMSS.json`** 到 **`.crabmate/exports/`**（或 `--output`）。fixture 顶层含 `version`、`source: "crabmate-tool-replay"`、可选 `note`、**`steps`**（`name`、`arguments`、`tool_call_id`、可选 **`recorded_output`**）。
- **`run`**：按 `steps` 顺序对当前工作区调用 **`tools::run_tool`**（**真实执行**：`run_command` / `http_fetch` 等仍受配置与白名单约束；**无**终端审批交互，非白名单 `run_command` 会直接失败）。`--compare-recorded` 时对含 `recorded_output` 的步骤做**字符串全等**比较，有不一致则进程退出码 **6**。

示例：

```bash
crabmate save-session --format json --workspace /path/to/proj   # 先得到 chat_export_*.json
crabmate tool-replay export --session-file /path/to/chat_export_20260101_120000.json --note "bug repro"
crabmate tool-replay run --fixture /path/to/proj/.crabmate/exports/tool_replay_20260101_120500.json
crabmate tool-replay run --fixture ./fixture.json --compare-recorded   # CI 回归
```

**安全**：重放与正常 Agent 回合相同，仅在**可信工作区**使用；勿对不可信会话 fixture 在敏感目录执行。

## `chat` 与管道

`--query`、`--stdin`、`--user-prompt-file` 三选一。`--system-prompt-file` 覆盖配置中的 system。`--messages-json-file` 提供单轮完整 messages。`--message-file` 为 JSONL 批跑。

**退出码**：**0** 成功；**1** 一般错误；**2** 用法错误；**3** 模型/解析失败；**4** 本回合所有 `run_command` 均被审批拒绝；**5** 配额/限流等（如 429）。

## CLI 内建命令

**启动摘要**：进入**交互式 CLI** 时于 stdout 打印分节说明——**模型**（含 `api_base` 截断、`llm_http_auth`、`temperature`、`llm_seed`、当前是否 **`--no-stream`**）、**工作区与工具**、**内建命令**列表、**要点配置**（如 `max_tokens`、`max_message_history`、API 超时/重试、`run_command` 超时与输出上限、分阶段规划、可选会话恢复/MCP/长期记忆等）。样式与 **`cli_repl_ui`** 的 `/help` 色阶一致；**`NO_COLOR`** 或非 TTY 下无 ANSI。运行中可随时输入 **`/config`** 再次打印**关键配置摘要**（字段与横幅要点同源并略扩展，**不**含密钥）。

**可选**：**`AGENT_CLI_WAIT_SPINNER=1`** 时，在等待模型**首包流式输出**（或 **`--no-stream`** 下整段 body）前于 **stderr** 显示 spinner 与已等待时间（默认关闭；须 stderr 为 TTY 且未设 **`NO_COLOR`**）。详见 **`docs/CONFIGURATION.md`**。

**分阶段规划（终端）**：若不想在**交互式 CLI** 打印**无工具规划轮**的模型原文，可设 **`staged_plan_cli_show_planner_stream = false`** 或 **`AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM=0`**（仍保留步骤队列摘要与后续执行步；见 **`docs/CONFIGURATION.md`**）。默认在首轮 **`agent_reply_plan`** 后还有一次无工具**步骤优化**轮，可用 **`staged_plan_optimizer_round = false`** 或 **`AGENT_STAGED_PLAN_OPTIMIZER_ROUND=0`** 关闭以省 API。

**SyncDefault Docker（CLI 与 `chat` 共用配置）**：可选将 **SyncDefault** 及部分工具（在宿主审批/白名单后）放入 **Docker** 执行（**`sync_default_tool_sandbox_mode = docker`**、镜像与 `user` 等；默认 Unix 上常用**当前有效 uid:gid** 以减轻工作区文件属主问题）。完整说明见 **`docs/CONFIGURATION.md`**「SyncDefault 工具 Docker 沙盒」。

**内建命令反馈样式**：成功/错误行首为 **✓** / **✗**；**`NO_COLOR`** 或非 TTY 下为 **`[ok]` / `[err]`**（纯 ASCII，避免缺字终端乱码）。

以 `/` 开头：**`/help`**、**`/clear`**、**`/model`**、**`/api-key`**（**`status` / `set <密钥>` / `clear`**；本进程内存中的 LLM Bearer 密钥，不写盘；别名 **`/apikey`**）、**`/config`**（无参数）、**`/doctor`**（同 **`crabmate doctor`**，无参数）、**`/probe`**（同 **`crabmate probe`**，无参数）、**`/models`** / **`/models list`**（同 **`crabmate models`**）、**`/models choose <id>`**（从当前 **`GET …/models`** 列表设**内存**中的 **`model`**，支持唯一不区分大小写前缀；持久化请改配置文件；**`/config reload`** 会从磁盘覆盖）、**`/agent`** / **`/agent list`**（先列内建 **`default`**，再列命名角色 id，并显示当前 REPL 选用；命名 id 与 **`GET /status`** 的 **`agent_role_ids`** 同源；未配置多角色时提示）、**`/agent set <id>`** 或 **`/agent set default`**（前者须 id 在角色表中；后者清除显式命名角色，与 Web 未选角色一致：`default_agent_role_id` 或全局 **`system_prompt`**）；二者均按新 system **重建首轮消息**，清空后续对话）、**`/workspace`** / **`/cd`**（**相对路径**与 **`read_file`** 等工具一致：相对**当前**工作区根、**禁止**以 **`/`** 开头的「伪相对」；**绝对路径**与 Web **`POST /workspace`** 一致：须落在 **`workspace_allowed_roots`** 且非敏感目录）、**`/tools`**、**`/export`**（可选参数 `json` / `markdown` / `both`，默认 `both`；导出**当前内存**）、**`/save-session`**（同上格式参数；从磁盘 **`tui_session.json`** 导出，同 **`crabmate save-session`**）。`quit` / `exit` / Ctrl+D 退出。

**Tab 补全**（交互 TTY、**reedline** 路径）：在「我:」提示下，若当前行（光标之前）去掉前导空白后以 **`/`** 开头，按 **Tab** 可弹出内建命令补全菜单（方向键或再按 **Tab** 选择；仅一项匹配时会直接填入）。**`/export`** 与 **`/save-session`** 在已输入完整命令名后再 **Tab** 可补 **`json` / `markdown` / `md` / `both`**。**`/mcp`** 后可补 **`list`**、**`probe`**、**`list probe`**（同 **`crabmate mcp list`** 语义）。**`/models`** 后可补 **`list`**、**`choose`**（**`choose`** 项末尾带空格，便于继续输入模型 id）。**`/agent`** 后可补 **`list`**、**`set`**（**`set`** 项末尾带空格，便于继续输入角色 id）。**`/api-key`** 与 **`/apikey`** 在根级补全列表中。在 **`bash#:`** 本地 shell 模式下关闭补全，避免干扰普通命令输入。

**`/mcp`**：只读列出本进程内 MCP stdio 缓存与合并后的 OpenAI 工具名（与 **`crabmate mcp list`** 一致）；**`/mcp probe`** 或 **`/mcp list probe`** 会按配置尝试连接一次（启动 **`mcp_command`** 子进程）。**`/version`**：打印 **`crabmate`** 版本与 **`OS`/`ARCH`**（不含密钥）。

**`/config reload`**：从 **`config.toml`** / **`.agent_demo.toml`**（或启动时 **`--config`** 指定文件）与当前进程环境变量重新合并配置，更新 **`api_base`、模型、超时、白名单、MCP、系统提示词文件重读** 等；**不**重建会话 SQLite 连接、**不**重建共享 **`reqwest::Client`**；**不**重读环境里的 **`API_KEY`**（**REPL** 下 **`/api-key`** 写入的内存密钥**不会**被清除）。Web 侧密钥仍随 **`client_llm.api_key`** 或进程启动时读入的 **`API_KEY`**。Web 等价：**`POST /config/reload`**（与受保护 API 相同鉴权规则）。若启动时挂了 Bearer 中间件，**清空/设置 token 后是否启用该中间件**仍须**重启 `serve`**。详见 **`docs/CONFIGURATION.md`**「配置热重载」。

**工具结果 stdout**：**交互式 CLI** / **`chat`**（无 SSE）下每轮工具执行后会打印 **`### 工具 · …`** 标题与正文。**`read_file`**、**`read_dir`**、**`list_tree`** 在终端均打印**摘要**（元数据/目录头/树参数块 + 条目或树行的前若干行，单行可截断），并注明完整输出在对话历史；其它工具仍打印正文（过长按配置截断）。完整工具结果均写入历史并供模型使用。若某工具结果判定为**失败**（如 `run_command` 非零退出、`错误：` 前缀等），终端会额外打印 **「自愈提示 · 诊断命令包」**：一行 JSON，可交给模型调用工具 **`playbook_run_commands`**（与 **`error_output_playbook`** 同源启发式，但会**真实执行**经白名单过滤的 `run_command`；**须先脱敏** `error_text`）。**不会**自动执行命令。

### 行首 `$`（本地 shell，安全边界）

在**交互式 TTY** 下，**当前输入为空**时按 **`$`**（或全角 **`＄`**，部分终端需 **Shift**）**无需 Enter** 即可在「我:」与 **`bash#:`** 间切换；仍支持**单独一行 `$` 后 Enter**。在 **`bash#:`** 下输入的一行会作为**本地 shell** 由本机 **`sh -c`**（Windows 为 **`cmd /C`**）在当前工作区目录运行，**不经过模型**，也**不受**模型工具 **`run_command` 白名单**限制——语义上等同于你在本机终端里自己敲的一条命令（可执行任意 `sh -c` 能表达的程序，仅 stdin 被置空）。行内已有字符时 **`$` 会正常插入**（用于对话里写美元金额等）。**仅应在可信本机 / 可信工作区使用**；需要受控命令集时请改用对话让模型走 `run_command`。管道/非 TTY 输入仍可用行内 `$ <命令>`。TTY 下编辑历史写入工作区 **`.crabmate/repl_history.txt`**（与模型会话文件不同）。

模型或网络等导致**本轮对话失败**时，**交互式 CLI** 会打印错误并**继续**等待下一行输入；若历史消息状态异常，可用 **`/clear`** 重置（保留当前 `system`）。

## `run_command` 终端审批

命令不在白名单时：**stdin** 与 **stderr** 均为 TTY 时，于 **stderr** 弹出 **dialoguer** 选项菜单（箭头键选择；设 **`NO_COLOR`** 时用无 ANSI 主题）；否则为**非交互回退**：打印说明后读一行，**y** 本次；**a** / **always** 本会话永久允许该命令名；**n** / 回车 拒绝（便于管道/CI 脚本 `echo y`）。**`chat --yes`** 对非白名单 **`run_command`** 以及未匹配前缀的 **`http_fetch` / `http_request`** 均直接放行（极危险）。**`chat --approve-commands a,b`** 仅额外允许列出的**命令名**（不作用于 HTTP 工具 URL）。

## CLI 与 Web 能力对照（会话持久 / 审批 / 导出）

以下说明**当前实现**下终端与 Web 的差异，避免误以为「CLI 与 Web 完全等价」。

| 能力 | Web（`serve`） | CLI |
|------|----------------|------------------------|
| **会话持久** | 可选 SQLite（`conversation_store_sqlite_path`）+ `conversation_id`，多会话、进程重启可续聊（受 TTL/条数上限等约束，见 `docs/DEVELOPMENT.md`）。 | **部分等价**：**交互式 CLI** 可选从工作区 **`.crabmate/tui_session.json`** 启动时加载/退出时保存（`tui_load_session_on_start` / `tui_session_max_messages`），为**单条会话链**文件，**不是** Web 的按 `conversation_id` 多会话库。`chat` 单次或批跑**不**自动跨命令持久化；需自行用 `--messages-json-file` 等传入上下文。另：**`repl_initial_workspace_messages_enabled`**（默认 false，见 `docs/CONFIGURATION.md`）为 true 时 **CLI** 才在后台构建 **`initial_workspace_messages`**（项目画像、依赖摘要，并参与上述磁盘恢复逻辑）；为 false 时启动仅一条 `system`，不跑 tokei / `cargo metadata` 等。 |
| **人工审批** | `run_command` 非白名单、**`http_fetch` / `http_request`**（未匹配 `http_fetch_allowed_prefixes`）等可走 SSE 控制面 + 浏览器 **`POST /chat/approval`**（非流式 `/chat` 无审批会话时仍拒绝）。 | **`run_command`**：见上一节（TTY 菜单 / 管道读行）。**`http_fetch` / `http_request`**：未匹配前缀时同样走该套审批；**`http_request`** 永久允许键为 **`http_request:<METHOD>:<URL>`**，与 **`http_fetch:`** 区分。 |
| **导出聊天记录** | 前端 **导出 JSON / Markdown**（与 `.crabmate/tui_session.json` 等形状对齐说明见 `README.md`）。 | **`save-session`**（兼容别名 **`export-session`**）从磁盘会话文件写入 **`.crabmate/exports/`**（与 Web 同形）；**交互式 CLI** **`/save-session`** 与之同逻辑；**交互式 CLI** **`/export`** 导出**当前内存**中的消息（未落盘的多轮也会写入）。`chat --output json` 仍仅辅助脚本输出本轮结构，**不等价**于完整会话导出文件。 |

本节与 `README.md` 随 CLI 导出能力变更时同步更新。

## 前端构建与 Web

```bash
cd frontend-leptos && trunk build && cd ..   # 开发（较快，不跑 wasm-opt）
# 发布或在意 WASM 体积：cd frontend-leptos && trunk build --release && cd ..
cargo run -- serve
```

静态资源由后端从 `frontend-leptos/dist` 提供。

## 主要 HTTP 路由（`serve`）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/` | 前端页面 |
| POST | `/config/reload` | 热重载内存中的 `AgentConfig`（不含会话 SQLite 路径）；body 可为 `{}`；见 **`docs/CONFIGURATION.md`**「配置热重载」 |
| POST | `/chat` | JSON 对话；可选 `conversation_id`、`agent_role`（仅新建服务端会话时）、`temperature`、`seed`、`seed_policy` |
| POST | `/chat/stream` | SSE；可选 `approval_session_id`、`agent_role`（同上）；响应头 `x-conversation-id` |
| POST | `/chat/approval` | 审批：`approval_session_id`、`decision` |
| POST | `/chat/branch` | 会话分叉截断：JSON `conversation_id`、`before_user_ordinal`（0-based 普通用户消息序号）、`expected_revision`；服务端截断到该序号对应用户消息**之前**（与 Web「从此处重试」一致：随后由 `/chat/stream` 再发同一条用户文本）。须已持久化会话且 `revision` 匹配 |
| GET | `/status` | 后台状态 |
| GET | `/workspace` | 工作区列表 |
| POST | `/workspace` | 设置当前 Web 工作区根：JSON `{"path":"/abs/dir"}`；省略 `path` 或空串恢复为默认（`run_command_working_dir`）；须为已存在目录且在 `workspace_allowed_roots` 内 |
| GET | `/workspace/pick` | 在**服务端进程所在机器**上弹出原生选目录对话框（`rfd`），返回 JSON `{"path":null}` 或 `{"path":"/chosen"}`；无图形/取消时多为 `null`；Web 侧栏「浏览…」取到路径后会紧接着 **`POST /workspace`** 自动生效 |
| GET | `/workspace/profile` | 项目画像 Markdown |
| GET | `/workspace/changelog` | 本会话工作区变更集 Markdown（可选查询 `conversation_id`；与 **`session_workspace_changelist`** 注入模型正文同源，只读） |
| GET | `/workspace/file` | 读工作区内文件（`path` 必填；可选 **`encoding`**，与工具 `read_file` 一致，默认 UTF-8 严格；单文件上限 1 MiB） |
| GET | `/health` | 健康检查 |

SSE 控制面字段见 **`docs/SSE_PROTOCOL.md`**。

## 打包 Debian `.deb`

```bash
cargo install cargo-deb
cd frontend-leptos && trunk build --release && cd ..
cargo build --release
cargo deb
sudo dpkg -i target/debian/crabmate_*.deb
```

安装后：`export API_KEY=… && crabmate serve 8080`。包内包含 **`/usr/share/man/man1/crabmate.1`**，可用 **`man crabmate`** 查看（若 **`MANPATH`** 已含 `/usr/share/man`）。

从源码树直接预览：`man -l man/crabmate.1`（路径相对仓库根）。
