# 命令行与子命令

查看帮助：`crabmate --help`、`crabmate help`、`crabmate help <子命令>`（与 `--help` 等价）。**全局选项**写在子命令**之前**：`--config`、`--workspace`、`--no-tools`、`--log`。

## 手册页（troff / `man`）

- **源码树**：预生成的 **`man/crabmate.1`**（troff），与当前 `clap` 定义一致；**Debian `.deb`** 会安装到 **`/usr/share/man/man1/crabmate.1`**（见根目录 `Cargo.toml` 的 `[package.metadata.deb] assets`）。
- **再生成**（增删子命令或全局选项后）：`cargo run --bin crabmate-gen-man`，然后提交更新后的 `man/crabmate.1`。
- **`cargo install`**：默认**不会**把 man 装到 `MANPATH`；可将 `man/crabmate.1` 拷到本机 `.../share/man/man1/` 后执行 `mandb`（视发行版而定），或优先使用 **`cargo deb`** / 发行版打包。

## 子命令一览

| 子命令 | 说明 |
|--------|------|
| `serve [PORT]` | Web UI + HTTP API，默认 **8080**；`serve --host <ADDR>` 绑定地址（默认 `127.0.0.1`）。`--no-web` / `--cli-only` 仅 API。 |
| `repl` | 交互式对话；**不写子命令时默认进入 repl**。 |
| `chat` | 单次/脚本对话：`--query` / `--stdin` / `--user-prompt-file`、`--system-prompt-file`、`--messages-json-file`、`--message-file`（JSONL）、`--yes` / `--approve-commands`、`--output json`、`--no-stream`。 |
| `bench` | 批量测评：`--benchmark`、`--batch` 等。 |
| `config` | 配置与 `API_KEY` 自检；`--dry-run` 可选。 |
| `doctor` | 本地诊断（**不需要** `API_KEY`）。 |
| `models` | `GET …/models`（需 `API_KEY`）。 |
| `probe` | 探测 models 端点（需 `API_KEY`）。 |
| `save-session` | 从会话文件导出 JSON/Markdown 到工作区 **`.crabmate/exports/`**（与 Web 导出同形；**不要**求 `API_KEY`）。`--format json|markdown|both`（默认 `both`），`--session-file` 可选。兼容别名 **`export-session`**。 |

## 日志级别

未设置 `RUST_LOG` 时：`serve` 默认 **info**；`repl` / `chat` / `bench` / `config` / `save-session`（及别名 `export-session`）默认 **warn**。可用 `RUST_LOG` 或 `--log <FILE>`。

## 消息管道调试日志

`RUST_LOG=crabmate=debug` 时每次调用模型前打印 **`message_pipeline session_sync`** 汇总；更细：`RUST_LOG=crabmate::message_pipeline=trace`。见 **`docs/DEVELOPMENT.md`** 与 `src/agent/message_pipeline.rs`。

## 兼容旧用法

未写子命令时仍可用 `--serve`、`--query`、`--benchmark`、`--dry-run` 等，内部映射为对应子命令。若参数中**任意位置**出现显式子命令名（如 `serve` / `doctor` / `save-session` / `export-session`），则整段 argv 不再插入默认 `repl`（与 `tests/fixtures/cli/legacy_normalize.json` 契约一致）。

## 常用选项（兼容写法）

| 选项 | 说明 |
|------|------|
| `--config <path>` | 指定配置文件（建议写在子命令前） |
| `--serve [port]` | 等价于 `serve` |
| `--host <ADDR>` | 随 `serve` |
| `--query` / `--stdin` | 等价于 `chat` |
| `--workspace <path>` | 覆盖初始工作区 |
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

默认读取 **`<workspace>/.crabmate/tui_session.json`**（`--workspace` 与全局 `--config` 写在子命令前），在 **`<workspace>/.crabmate/exports/`** 下生成带时间戳的 **`chat_export_*.json`** / **`chat_export_*.md`**（与 Web 前端导出约定一致，见 `runtime/chat_export.rs` 与 `frontend/src/chatExport.ts`）。每行 stdout 为写出文件的绝对路径，便于脚本捕获。

## `chat` 与管道

`--query`、`--stdin`、`--user-prompt-file` 三选一。`--system-prompt-file` 覆盖配置中的 system。`--messages-json-file` 提供单轮完整 messages。`--message-file` 为 JSONL 批跑。

**退出码**：**0** 成功；**1** 一般错误；**2** 用法错误；**3** 模型/解析失败；**4** 本回合所有 `run_command` 均被审批拒绝；**5** 配额/限流等（如 429）。

## REPL 内建命令

**启动摘要**：进入 REPL 时于 stdout 打印分节说明——**模型**（含 `api_base` 截断、`llm_http_auth`、`temperature`、`llm_seed`、当前是否 **`--no-stream`**）、**工作区与工具**、**内建命令**列表、**要点配置**（如 `max_tokens`、`max_message_history`、API 超时/重试、`run_command` 超时与输出上限、分阶段规划、可选会话恢复/MCP/长期记忆等）。样式与 **`cli_repl_ui`** 的 `/help` 色阶一致；**`NO_COLOR`** 或非 TTY 下无 ANSI。运行中可随时输入 **`/config`** 再次打印**关键配置摘要**（字段与横幅要点同源并略扩展，**不**含密钥）。

**可选**：**`AGENT_CLI_WAIT_SPINNER=1`** 时，在等待模型**首包流式输出**（或 **`--no-stream`** 下整段 body）前于 **stderr** 显示 spinner 与已等待时间（默认关闭；须 stderr 为 TTY 且未设 **`NO_COLOR`**）。详见 **`docs/CONFIGURATION.md`**。

以 `/` 开头：**`/help`**、**`/clear`**、**`/model`**、**`/config`**（无参数）、**`/doctor`**（同 **`crabmate doctor`**，无参数）、**`/probe`**（同 **`crabmate probe`**，无参数）、**`/models`**（同 **`crabmate models`**，无参数）、**`/workspace`** / **`/cd`**、**`/tools`**、**`/export`**（可选参数 `json` / `markdown` / `both`，默认 `both`；导出**当前内存**）、**`/save-session`**（同上格式参数；从磁盘 **`tui_session.json`** 导出，同 **`crabmate save-session`**）。`quit` / `exit` / Ctrl+D 退出。

**Tab 补全**（交互 TTY、**reedline** 路径）：在「我:」提示下，若当前行（光标之前）去掉前导空白后以 **`/`** 开头，按 **Tab** 可弹出内建命令补全菜单（方向键或再按 **Tab** 选择；仅一项匹配时会直接填入）。**`/export`** 与 **`/save-session`** 在已输入完整命令名后再 **Tab** 可补 **`json` / `markdown` / `md` / `both`**。在 **`bash#:`** 本地 shell 模式下关闭补全，避免干扰普通命令输入。

**工具结果 stdout**：REPL / **`chat`**（无 SSE）下每轮工具执行后会打印 **`### 工具 · …`** 标题与正文摘要。**`read_file`** 与 **`list_tree`** 仅打印标题与一行省略说明，**不**回显工具返回正文（避免大文件/目录树刷屏）；完整结果仍写入对话历史并供模型使用。

### 行首 `$`（本地 shell，安全边界）

在**交互式 TTY** 下，**当前输入为空**时按 **`$`**（或全角 **`＄`**，部分终端需 **Shift**）**无需 Enter** 即可在「我:」与 **`bash#:`** 间切换；仍支持**单独一行 `$` 后 Enter**。在 **`bash#:`** 下输入的一行会作为**本地 shell** 由本机 **`sh -c`**（Windows 为 **`cmd /C`**）在当前工作区目录运行，**不经过模型**，也**不受**模型工具 **`run_command` 白名单**限制——语义上等同于你在本机终端里自己敲的一条命令（可执行任意 `sh -c` 能表达的程序，仅 stdin 被置空）。行内已有字符时 **`$` 会正常插入**（用于对话里写美元金额等）。**仅应在可信本机 / 可信工作区使用**；需要受控命令集时请改用对话让模型走 `run_command`。管道/非 TTY 输入仍可用行内 `$ <命令>`。TTY 下编辑历史写入工作区 **`.crabmate/repl_history.txt`**（与模型会话文件不同）。

模型或网络等导致**本轮对话失败**时，REPL 会打印错误并**继续**等待下一行输入；若历史消息状态异常，可用 **`/clear`** 重置（保留当前 `system`）。

## `run_command` 终端审批

命令不在白名单时：**stdin** 与 **stderr** 均为 TTY 时，于 **stderr** 弹出 **dialoguer** 选项菜单（箭头键选择；设 **`NO_COLOR`** 时用无 ANSI 主题）；否则为**非交互回退**：打印说明后读一行，**y** 本次；**a** / **always** 本会话永久允许该命令名；**n** / 回车 拒绝（便于管道/CI 脚本 `echo y`）。**`chat --yes`** 对非白名单 **`run_command`** 以及未匹配前缀的 **`http_fetch` / `http_request`** 均直接放行（极危险）。**`chat --approve-commands a,b`** 仅额外允许列出的**命令名**（不作用于 HTTP 工具 URL）。

## CLI 与 Web 能力对照（会话持久 / 审批 / 导出）

以下说明**当前实现**下终端与 Web 的差异，避免误以为「CLI 与 Web 完全等价」。

| 能力 | Web（`serve`） | CLI（`repl` / `chat`） |
|------|----------------|------------------------|
| **会话持久** | 可选 SQLite（`conversation_store_sqlite_path`）+ `conversation_id`，多会话、进程重启可续聊（受 TTL/条数上限等约束，见 `docs/DEVELOPMENT.md`）。 | **部分等价**：REPL 可选从工作区 **`.crabmate/tui_session.json`** 启动时加载/退出时保存（`tui_load_session_on_start` / `tui_session_max_messages`），为**单条会话链**文件，**不是** Web 的按 `conversation_id` 多会话库。`chat` 单次或批跑**不**自动跨命令持久化；需自行用 `--messages-json-file` 等传入上下文。 |
| **人工审批** | `run_command` 非白名单、**`http_fetch` / `http_request`**（未匹配 `http_fetch_allowed_prefixes`）等可走 SSE 控制面 + 浏览器 **`POST /chat/approval`**（非流式 `/chat` 无审批会话时仍拒绝）。 | **`run_command`**：见上一节（TTY 菜单 / 管道读行）。**`http_fetch` / `http_request`**：未匹配前缀时同样走该套审批；**`http_request`** 永久允许键为 **`http_request:<METHOD>:<URL>`**，与 **`http_fetch:`** 区分。 |
| **导出聊天记录** | 前端 **导出 JSON / Markdown**（与 `.crabmate/tui_session.json` 等形状对齐说明见 `README.md`）。 | **`save-session`**（兼容别名 **`export-session`**）从磁盘会话文件写入 **`.crabmate/exports/`**（与 Web 同形）；REPL **`/save-session`** 与之同逻辑；REPL **`/export`** 导出**当前内存**中的消息（未落盘的多轮也会写入）。`chat --output json` 仍仅辅助脚本输出本轮结构，**不等价**于完整会话导出文件。 |

本节与 `README.md` 随 CLI 导出能力变更时同步更新。

## 前端构建与 Web

```bash
cd frontend && npm install && npm run build && cd ..
cargo run -- serve
```

静态资源由后端从 `frontend/dist` 提供。

## 主要 HTTP 路由（`serve`）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/` | 前端页面 |
| POST | `/chat` | JSON 对话；可选 `conversation_id`、`temperature`、`seed`、`seed_policy` |
| POST | `/chat/stream` | SSE；可选 `approval_session_id`；响应头 `x-conversation-id` |
| POST | `/chat/approval` | 审批：`approval_session_id`、`decision` |
| POST | `/chat/branch` | 会话分叉截断（见开发文档） |
| GET | `/status` | 后台状态 |
| GET | `/workspace` | 工作区列表 |
| GET | `/workspace/profile` | 项目画像 Markdown |
| GET | `/workspace/file` | 读工作区内文件（`path` 必填；可选 **`encoding`**，与工具 `read_file` 一致，默认 UTF-8 严格；单文件上限 1 MiB） |
| GET | `/health` | 健康检查 |

SSE 控制面字段见 **`docs/SSE_PROTOCOL.md`**。

## 打包 Debian `.deb`

```bash
cargo install cargo-deb
cd frontend && npm install && npm run build && cd ..
cargo build --release
cargo deb
sudo dpkg -i target/debian/crabmate_*.deb
```

安装后：`export API_KEY=… && crabmate serve 8080`。包内包含 **`/usr/share/man/man1/crabmate.1`**，可用 **`man crabmate`** 查看（若 **`MANPATH`** 已含 `/usr/share/man`）。

从源码树直接预览：`man -l man/crabmate.1`（路径相对仓库根）。
