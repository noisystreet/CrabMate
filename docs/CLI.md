# 命令行与子命令

查看帮助：`crabmate --help`、`crabmate help`、`crabmate help <子命令>`（与 `--help` 等价）。**全局选项**写在子命令**之前**：`--config`、`--workspace`、`--no-tools`、`--log`。

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

## 日志级别

未设置 `RUST_LOG` 时：`serve` 默认 **info**；`repl` / `chat` / `bench` / `config` 默认 **warn**。可用 `RUST_LOG` 或 `--log <FILE>`。

## 消息管道调试日志

`RUST_LOG=crabmate=debug` 时每次调用模型前打印 **`message_pipeline session_sync`** 汇总；更细：`RUST_LOG=crabmate::message_pipeline=trace`。见 **`docs/DEVELOPMENT.md`** 与 `src/agent/message_pipeline.rs`。

## 兼容旧用法

未写子命令时仍可用 `--serve`、`--query`、`--benchmark`、`--dry-run` 等，内部映射为对应子命令。

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
```

## `chat` 与管道

`--query`、`--stdin`、`--user-prompt-file` 三选一。`--system-prompt-file` 覆盖配置中的 system。`--messages-json-file` 提供单轮完整 messages。`--message-file` 为 JSONL 批跑。

**退出码**：**0** 成功；**1** 一般错误；**2** 用法错误；**3** 模型/解析失败；**4** 本回合所有 `run_command` 均被审批拒绝；**5** 配额/限流等（如 429）。

## REPL 内建命令

以 `/` 开头：**`/help`**、**`/clear`**、**`/model`**、**`/workspace`** / **`/cd`**、**`/tools`**。行首 **`$`**（TTY 下提示变为 `bash#:`）执行本地 shell 一行，**不**经 `run_command` 白名单，**仅可信工作区**使用。`quit` / `exit` / Ctrl+D 退出。

## `run_command` 终端审批

命令不在白名单时 stdin 确认：**y** 本次；**a** / **always** 本会话永久允许该命令名；**n** / 回车 拒绝。**`chat --yes`** 全放行（极危险）。**`chat --approve-commands a,b`** 额外允许列出的命令名。

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

安装后：`export API_KEY=… && crabmate serve 8080`。
