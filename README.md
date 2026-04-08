**语言 / Languages:** 中文（本页）· [English](README-en.md)

# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate Logo" width="240" />
</p>

**CrabMate** 是基于 Rust 编写的 AI Agent，通过 **OpenAI 兼容** 的 `chat/completions` 对接 DeepSeek、MiniMax、智谱 GLM、Moonshot Kimi、本地 Ollama 等后端大模型；内置 **Function Calling** 以及工作区内的命令、文件等工具，并提供 **Web UI** 与 **CLI**。

## 目录

- [CrabMate](#crabmate)
  - [目录](#目录)
  - [功能概览](#功能概览)
  - [文档索引](#文档索引)
  - [后端模型支持](#后端模型支持)
  - [环境与快速开始](#环境与快速开始)
  - [源码编译与打包](#源码编译与打包)
  - [部署与安全](#部署与安全)
  - [项目结构](#项目结构)
  - [参考](#参考)

## 功能概览

- **对话与多模型**：OpenAI 兼容 `chat/completions`；切换模型见配置。
- **内置工具**：文件、命令、HTTP、联网搜索、多语言开发辅助（含 **JVM**：Maven/Gradle；**容器**：`docker`/`podman`/`docker compose` 最小封装）等；**能力与 JSON 参数示例**见 [docs/TOOLS.md](docs/TOOLS.md)。另有结构化内置工具 **`gh_pr_*`（含 `gh_pr_diff`）、`gh_issue_*`、`gh_run_*`（含 `gh_run_view`）、`gh_release_*`、`gh_search`、`gh_api`**（见 [docs/TOOLS.md](docs/TOOLS.md)）。默认 `run_command` 白名单含 **GitHub CLI `gh`**（需本机安装并完成认证，参数规则同其他白名单命令：不得含 `..`、不得以 `/` 开头）；未安装时 **`GET /health`** 中 **`dep_gh`** 为 degraded，**`crabmate doctor`** 亦会提示。可选配置 **`health_llm_models_probe`**（或 **`AGENT_HEALTH_LLM_MODELS_PROBE`**）：在 **`/health`** 中对当前 **`api_base`** 发起 **GET …/models**（与 **`crabmate probe`** 同源，无 completion 费用），并可用 **`health_llm_models_probe_cache_secs`** 限制探活频率；详见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。`cargo_test` / `npm run test` 以及部分 `run_command cargo test`：进程内可按「源码指纹 + 参数」复用上一次的截断输出，并标注 **缓存命中**（`test_result_cache_*`，见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)）。
- **CLI（终端）**：默认 `cargo run` / `crabmate repl` 进入**交互式 CLI**（流式对话、内建命令如 `/config` / `/doctor` / `/probe` / `/models`（含 `/models choose` 从列表设当前模型）、可选 `bash#:` 一行 shell）；`crabmate chat` 适合脚本与管道的**单次提问**；`crabmate serve` 与 Web 共用同一套 Agent 与工具逻辑。另有 `doctor`（环境自检）、`config`（配置干跑）、`bench`、`save-session` / `export-session`、`tool-replay`、`mcp list` 等子命令，全局选项含 `--config`、`--workspace`、`--agent-role`（多角色首条 system，见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)）、`--no-tools`、`--no-stream` 等。完整列表、退出码与 `man crabmate` 见 [docs/CLI.md](docs/CLI.md)。
- **Web UI**：布局参考 DeepSeek Chat（左侧会话栏 + 居中对话 + 底部圆角输入；**助手**普通回复渲染 **Markdown**，含多级标题、列表、代码块与表格等，经净化后展示）；**首次发送**首条用户消息后，若标题仍为默认「新会话」且未事先重命名，侧栏标题会自动改为该条提问的截断摘要；左侧最近会话可**右键**导出 JSON/Markdown 或删除（与「管理会话」内一致）；聊天（新会话无消息时居中显示操作引导）、工作区浏览/编辑（侧栏文件列表**目录默认折叠**，展开后按需请求子路径；侧栏可 **`GET /workspace/pick`** 在运行 serve 的机器上系统选目录并**自动** **`POST /workspace`** 生效（手输路径后按 **Enter** 提交），须符合 `workspace_allowed_roots`；`GET /workspace/file` 可选 `encoding` 查询参数，语义与 `read_file` 一致，便于 GBK/GB18030 等 legacy 文本）、任务清单（进程内 `/tasks`，重启后清空）、状态栏；Agent 写入文件后，工作区列表会自动刷新；工作区卡片底部可打开**变更预览**弹层，经 **`GET /workspace/changelog`** 展示与 **`session_workspace_changelist`** 注入模型同源的 **unified diff** 摘要（弹层打开时若收到 `workspace_changed` 会自动刷新）。侧栏支持按标题**筛选会话**、在全部本地会话中**搜索消息**（内存扫描，最多展示 80 条命中，点击可跳转并滚动到该条）；主区提供**当前会话内查找**与上/下条导航；在消息区等（非输入框 / 查找框焦点）可按 **End** 滚至最新消息并恢复自动跟底；在**聊天消息区右键**打开菜单：**在同一消息气泡内选中部分文字后**可出现**复制选中文字**；亦可进入多选；主区搜索图标左侧亦有**多选**图标可切换多选。勾选后在右键菜单中**导出已选为 Markdown**（与整会话导出规则一致，按会话内时间顺序）。气泡上支持**复制**、**长回复折叠/展开**；对普通用户消息可选**从此处重试**（需服务端已持久化 `conversation_id` 且 SSE 已下发 `revision` 时同步 `POST /chat/branch`）与**分支对话**（截断本地与会话存储）。可选 **多角色**：配置 `[[agent_roles]]` 或 `config/agent_roles.toml` 后，新会话首条请求可带 `agent_role`（或 `default_agent_role` / `AGENT_DEFAULT_AGENT_ROLE`）；仓库默认附带 **`companion`（陪聊）**、**`scientist`（科学家向）**、**`engineer`（工程师向）**、**`philosopher`（哲学向）**、**`literary`（文学向）**、**`code_reviewer`（代码审阅）** 等角色，见 `config/agent_roles.toml` 与 `config/prompts/*_system_prompt.md`。详见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)「多角色」。
- **项目画像**：侧栏只读摘要（`Cargo.toml` / `package.json`、目录与 tokei 等）；可与工作区备忘合并注入新会话首轮（`project_profile_inject_*`）。另可选注入 **cargo metadata** 解析的 workspace 内 crate 依赖图（Mermaid + 结构化 JSON）与仓库根 `package.json` 的依赖名节选（`project_dependency_brief_inject_*`，与 Web / `repl` / `chat` 首轮同源）。模型也可用内置工具 `repo_overview_sweep` 拉取同源画像（见 `include_project_profile` / `project_profile_max_chars`，[docs/TOOLS.md](docs/TOOLS.md)）。
- **OpenAPI**：`serve` 暴露 **`GET /openapi.json`**（OpenAPI 3.0，`application/json`），汇总主要 HTTP 路径与 JSON 形状，便于 CI / 客户端生成代码；**`/chat/stream` 的 SSE 行协议**仍以 [docs/SSE_PROTOCOL.md](docs/SSE_PROTOCOL.md) 为准。
- **流式与审批**：Web SSE；`run_command` 与未匹配前缀的 `http_fetch` / `http_request` 等可走 `POST /chat/approval`。客户端断开或协作取消时，在 SSE 仍可投递的情况下可能收到控制面 `error` + `code: STREAM_CANCELLED`（与协议表见 [docs/SSE_PROTOCOL.md](docs/SSE_PROTOCOL.md)）。CLI 下非白名单 `run_command` 与未匹配前缀的 `http_fetch` / `http_request` 走同一套终端审批（TTY 为 dialoguer 菜单，管道/无头读一行 `y` / `a` / `n`；或 `--yes` / `--approve-commands`，后者仅命令名）。**Web 与 CLI 对照表**见 [docs/CLI.md](docs/CLI.md)「CLI 与 Web 能力对照」。
- **会话与导出**：Web 可选 `conversation_id` + `conversation_store_sqlite_path` 持久化（TTL/条数上限见配置），并可在 UI **导出 JSON/MD**。CLI 可用 `crabmate save-session`（兼容别名 `export-session`；默认读工作区 `.crabmate/tui_session.json`，写入 `.crabmate/exports/`，与前端同形），交互式 CLI 中 `/save-session`（与上述子命令同逻辑）或 `/export`（导出当前内存中的对话）。`crabmate tool-replay` 可从会话 JSON 提取工具调用序列为 fixture 并重放（复现/回归，不调用大模型；见 [docs/CLI.md](docs/CLI.md)）。交互式 CLI 可选从 `tui_session.json` 恢复（`tui_load_session_on_start`）；默认**不**在后台构建 `initial_workspace_messages`（无首轮项目画像/依赖摘要注入），需设 `repl_initial_workspace_messages_enabled = true`（或 `AGENT_REPL_INITIAL_WORKSPACE_MESSAGES_ENABLED`）方启用。默认按会话累积**工具写入路径 + unified diff 摘要**并在每轮请求模型前注入（`session_workspace_changelist_*`，保存会话前剥离）。备忘 `agent_memory_file`、长期记忆见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。会话/审批差异仍以 [docs/CLI.md](docs/CLI.md) 对照节为准。
- **可选 MCP（stdio）**：配置 `mcp_enabled` + `mcp_command` 后合并远端工具为 `mcp__{slug}__{tool}`；同一进程内复用一条 stdio 连接（`serve` / `repl` / `chat` 多轮共用）。运维可 `crabmate mcp list` 查看本进程已缓存的会话与合并后的工具名（**不**需要 `API_KEY`）；`mcp list --probe` 会按配置尝试连接一次（用于排障，会启动 `mcp_command` 子进程）。`mcp_command` 会启动子进程，请在可信环境下配置。

## 文档索引

| 文档                                               | 内容                                           | English |
| ------------------------------------------------ | -------------------------------------------- | ------- |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)       | 架构、模块索引、协议与扩展点                               | [en](docs/en/DEVELOPMENT.md) |
| [docs/TOOLS.md](docs/TOOLS.md)                   | 内置工具说明与调用示例                                  | [en](docs/en/TOOLS.md) |
| [docs/SSE_PROTOCOL.md](docs/SSE_PROTOCOL.md)     | `/chat/stream` 控制面 JSON                      | [en](docs/en/SSE_PROTOCOL.md) |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md)   | 环境变量、`AGENT_*`、规划/上下文等配置详解                   | [en](docs/en/CONFIGURATION.md) |
| [docs/CLI.md](docs/CLI.md)                       | 子命令、选项、HTTP 路由、打包 deb                        | [en](docs/en/CLI.md) |
| [docs/CLI_CONTRACT.md](docs/CLI_CONTRACT.md)     | `chat` 退出码、`--output json` 行协议、与 SSE 错误码交叉引用 | [en](docs/en/CLI_CONTRACT.md) |
| [docs/TODOLIST.md](docs/TODOLIST.md)             | 未完成待办：全局 P0–P5 + 按模块分章（完成后从清单删除）             | [en](docs/en/TODOLIST.md) |
| [docs/CODEBASE_INDEX_PLAN.md](docs/CODEBASE_INDEX_PLAN.md) | 统一代码索引与增量缓存规划 | [en](docs/en/CODEBASE_INDEX_PLAN.md) |

维护约定：用户可见变更需同步 README、[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)、[docs/TOOLS.md](docs/TOOLS.md) 等，见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)「TODOLIST 与功能文档约定」。

## 后端模型支持

CrabMate 使用 `POST {api_base}/chat/completions`（OpenAI 兼容形态，含可选流式 SSE、tools / tool_calls；具体能力以各供应商为准）。通过 `[agent]` 段的 `api_base`、`model` 与 `llm_http_auth_mode` 切换网关；密钥仅通过环境变量 `API_KEY` 传入（`bearer` 时），**不要**把真实密钥写入仓库配置文件。


| 场景              | 配置要点                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **DeepSeek**    | `api_base`：`https://api.deepseek.com/v1`；`model`（已测）：`deepseek-chat`（通用对话）与 `deepseek-reasoner`（推理模型，响应可含 `reasoning_content` 思维链）；具体以 [DeepSeek 开放平台](https://platform.deepseek.com/) 当前可用模型名为准。参考 [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)。                                                                                                                                                                                                                            |
| **MiniMax**     | `api_base`：`https://api.minimaxi.com/v1`；`model`（已测）：`MiniMax-M2.7`、`MiniMax-M2.7-highspeed`、`MiniMax-M2.5`；其它型号以控制台为准。线上常见 `invalid message role: system`，程序会按 **`model` / `api_base`** 识别 MiniMax 并**自动**将系统提示并入 **`user`**。**未写 `llm_reasoning_split` 时，MiniMax 网关默认开启思维链分离**（等价 `reasoning_split: true`）；不需要时可显式 `llm_reasoning_split = false` 或 `AGENT_LLM_REASONING_SPLIT=0`。说明与示例见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)「MiniMax」与 [MiniMax OpenAI 兼容](https://platform.minimaxi.com/docs/api-reference/text-openai-api)。 |
| **智谱 GLM**    | `api_base`：`https://open.bigmodel.cn/api/paas/v4`；`model` 如 `glm-5`；`API_KEY`（Bearer）。默认与官方最小 cURL（`model` + `messages` + `stream: true`，无 `thinking`）一致。可选 **深度思考**：`llm_bigmodel_thinking = true`（`AGENT_LLM_BIGMODEL_THINKING`）。详见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)「智谱 GLM」与 [GLM-5 文档](https://docs.bigmodel.cn/cn/guide/models/text/glm-5)。 |
| **Moonshot Kimi** | `api_base`：`https://api.moonshot.cn/v1`；`model` 如 `kimi-k2.5` / `kimi-k2-0905-preview`（以 [Kimi API 文档](https://platform.moonshot.cn/docs/api/chat) 为准）；`API_KEY`。出站 `temperature` 按模型 ID 自动钳制（**k2.5** 与 **k2-thinking → 1**；其它 **k2（非上两类）→ 0.6** 等，避免 400）。可选关闭 **k2.5** 默认思考：`llm_kimi_thinking_disabled = true`（`AGENT_LLM_KIMI_THINKING_DISABLED`）。详见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)「Moonshot（Kimi）」。 |
| **本地 Ollama 等** | `api_base` 如 `http://127.0.0.1:11434/v1`；`llm_http_auth_mode = "none"`（或 `AGENT_LLM_HTTP_AUTH_MODE=none`），可不设 `API_KEY`。工具调用是否稳定取决于模型与 Ollama 版本。                                                                                                                                                                                                                                                                                                                                                                         |


在本机可运行 `crabmate doctor`（**不**需要 `API_KEY`）、`crabmate probe`、`crabmate models`，检查鉴权模式、连通性及 `GET …/models` 列表（`bearer` 模式下会携带 Bearer）。完整的 `AGENT_*`、热重载与边界说明见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。

**具体模型的能力、限制与调用方式**（如可用模型 ID、采样参数取值、多模态、供应商专有字段等），**请以各模型厂商官方网站公布的 API 文档为准**；本节与项目内配置文档仅说明在 CrabMate 中如何填写 `api_base` / `model` 等对接项。

## 环境与快速开始

- **Rust**：1.85+（edition 2024，见 [AGENTS.md](AGENTS.md)）
- **Docker 开发环境**（可选）：仓库根目录 [Dockerfile](Dockerfile) 基于 **Ubuntu 24.04**，预装 stable Rust、`wasm32-unknown-unknown`、`rustfmt` / `clippy`、`trunk`，以及 `libssl-dev`、`libssh2-1-dev`、`g++`（与 `.cargo/config.toml` 中 Linux 链接说明一致）等。示例：

  ```bash
  docker build -t crabmate-dev .
  docker run --rm -it -v "$(pwd)":/workspace -w /workspace crabmate-dev
  ```

  宿主机 UID/GID 非 1000 时，可 `docker build --build-arg DEV_UID="$(id -u)" --build-arg DEV_GID="$(id -g)" -t crabmate-dev .` 再运行。容器内按需执行 `cargo build`、`cargo test`、`cd frontend-leptos && trunk build` 等；**未**预装 `pre-commit` / Node（若需前端以外的 npm 脚本可自行 `apt` 或另层镜像）。
- **环境变量**：`API_KEY` — 云端 OpenAI 兼容网关的 Bearer 密钥（`llm_http_auth_mode=bearer` 时；**`serve` / `repl` / `chat` 可在未 export 时启动**，对话前请在 **Web 侧栏「设置」**填写密钥，或 **REPL** 执行 **`/api-key set <密钥>`**；**`crabmate models` / `probe`** 在 bearer 下仍要求环境变量非空）。`doctor` / `save-session` 等不要求 `API_KEY`。`AGENT_API_BASE`、`AGENT_MODEL` — 分别覆盖配置中的 `api_base`、`model`。可选 **`CRABMATE_REQUEST_CHROME_TRACE_DIR`** …（详见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)）。完整 `AGENT_*` 表亦见该文档。

```bash
# 可选：设置你所用的模型API的base_url和模型名称（参考模型官方网站的文档）
# export AGENT_API_BASE="https://api.deepseek.com/v1"
# export AGENT_MODEL="deepseek-chat"

# 可选：bearer 时可在启动前 export，或在 Web「设置」/ REPL `/api-key set …` 再对话
# export API_KEY="your-api-key"
cargo b #编译
cargo run              # 默认进入 cli终端界面（bearer 且无密钥时会提示 /api-key）

#启动 server和web界面
cd frontend-leptos
trunk build            # 开发：较快，不跑 wasm-opt
# 发布或在意 WASM 体积时：trunk build --release
cd ..
cargo run -- serve     # 可先启动再在浏览器侧栏填写 API 密钥；默认 8080
```

**CLI**：`cargo run`（省略子命令）默认进入**交互式终端对话**；启动时打印模型、工作区与内建命令等**分节摘要**，运行中以 `/` 开头的命令（如 `/config`、`/config reload`、`/api-key`、`/doctor`、`/probe`、`/models`、`/mcp`、`/version`）及可选 `bash#:` 本地一行 shell 等，详见 [docs/CLI.md](docs/CLI.md)「CLI 内建命令」（含 Tab 补全、`$` 切换、分阶段规划终端输出、**SyncDefault Docker**、等待 **spinner**、`✓` / `[ok]` 反馈样式等）。对应配置键见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。

**前端**：执行 `cd frontend-leptos && trunk build`（输出 `frontend-leptos/dist`）后启动 `serve`。默认 **`trunk build`** 为开发构建（不跑 `wasm-opt`）；**发布或打包**请用 **`trunk build --release`** 以启用默认 `wasm-opt`、减小 WASM（见 **`frontend-leptos/README.md`** / **`docs/DEVELOPMENT.md`**）。**界面语言**在右列「**设置**」中可选 **简体中文 / English**（本机 `localStorage`：`crabmate-locale`）；在非输入框焦点下按 **Esc** 可依次关闭顶层弹层（菜单、查找栏、模态框等）。

**配置**：`config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml`（编译嵌入）+ 可选 `config.toml`；默认通过 `system_prompt_file = "config/prompts/default_system_prompt.md"` 从仓库文件加载（**改该文件无需重编**；相对路径会按当前目录、配置文件所在目录、`run_command_working_dir` 依次解析，见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)）。**环境变量与高级项**同见该文档。**子命令与 Benchmark** 见 [docs/CLI.md](docs/CLI.md)；**release 构建、`cargo deb`、`man` 页**见下文 **[「源码编译与打包」](#源码编译与打包)**。

**切换模型 / 网关**（DeepSeek、MiniMax、Ollama 等）：见上文 **[「后端模型支持」](#后端模型支持)**。

## 源码编译与打包

- **工具链**：后端需 **Rust 1.85+**（edition 2024）；构建 Web 前端需 **Trunk**（`cargo install trunk`）与 `wasm32-unknown-unknown` 目标。Linux 上系统库与链接说明（如 `libssl-dev`、`libssh2`，以及长期记忆、可选 ONNX / `g++` 等）见 [AGENTS.md](AGENTS.md)。
- **Debug 版本构建**：仓库根目录执行 `cargo build`，二进制位于 `target/debug/crabmate`。
- **Release 版本构建**：`cargo build --release`，二进制位于 `target/release/crabmate`。若以 `serve` 提供 Web UI，先执行 `cd frontend-leptos && trunk build --release`（输出 `frontend-leptos/dist`，含默认 `wasm-opt`），再启动后端。
- **检查与测试**（维护者/CI）：`cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`。仓库 pre-commit 配置见 [.pre-commit-config.yaml](.pre-commit-config.yaml)。
- **安装到本机**：`cargo install --path .`（在克隆目录下；默认安装 release 二进制到 `~/.cargo/bin`）。`cargo install` 不会自动安装 `man` 页；可手动安装 [man/crabmate.1](man/crabmate.1) 或优先使用下方 `.deb`。
- **手册页**：`clap` 与 troff 不同步时，在仓库根执行 `cargo run --bin crabmate-gen-man` 再提交更新后的 [man/crabmate.1](man/crabmate.1)。
- 生成 **Debian `.deb` 包**：需安装 [cargo-deb](https://github.com/kornelski/cargo-deb)（`cargo install cargo-deb`），先执行 `cd frontend-leptos && trunk build --release`，再 `cargo build --release` 与 `cargo deb`；生成的包在 `target/debian/`，安装示例 `sudo dpkg -i target/debian/crabmate_*.deb`。包内附带默认配置片段、[README.md](README.md) 与 `/usr/share/man/man1/crabmate.1`。细则与路由表见 [docs/CLI.md](docs/CLI.md)「打包 Debian `.deb`」。

## 部署与安全

- **默认仅本机**：`serve` 绑定 `127.0.0.1`。监听 `0.0.0.0` 时须配置 `web_api_bearer_token`，或显式开启 `allow_insecure_no_auth_for_non_loopback`（**不安全**，仅建议在可信网络下临时使用）。
- **Bearer**：设置后主要 API 需 `Authorization: Bearer`；前端可从 `localStorage["crabmate-api-bearer-token"]` 读取。
- **Web「设置」— 模型覆盖（可选）**：可在本机保存 **`api_base` / `model` / `API_KEY`**（密钥框为密码输入、不回显已存密钥），随 **`POST /chat/stream`** 以 JSON 字段 **`client_llm`** 发送，仅影响当次对话任务，不修改服务端 TOML。详见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)「Web 对话队列」。
- **工作区路径**：须在允许的根目录内；每次请求重验。未配白名单时仅允许 `run_command_working_dir` 下路径。无鉴权时不要暴露在不可信网络。**路径安全为尽力校验**：在 **Unix** 上，内置 `read_file` 与 Web 工作区文件/列表等优先经 **`openat2` + `RESOLVE_IN_ROOT`（Linux）** 在已校验根 fd 上打开，收窄「先校验再按路径打开」的窗口；**仍非**内核级不可逃逸保证（策略校验仍依赖 `canonicalize` 时刻，部分工具路径未全部贯通）。说明见 [`src/path_workspace.rs`](src/path_workspace.rs)、[`src/workspace_fs.rs`](src/workspace_fs.rs) 与 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)「工作区与 Cursor 式规则」。
- **联网搜索 Key**：`web_search_api_key` 与主对话所用 `API_KEY` 分离，注意文件权限。
- **可选 Docker 工具沙盒**：将 SyncDefault 与部分工具（含 `run_command` 等，在宿主白名单/审批后）放到一次性容器内执行；需本机 Docker、**自管镜像**（镜像提供 CLI 依赖，宿主 `crabmate` 二进制只读挂入容器）。完整步骤、镜像要求、网络与 `user` 见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)「SyncDefault 工具 Docker 沙盒」。

更细的边界与敏感面见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) 与 [.cursor/rules/security-sensitive-surface.mdc](.cursor/rules/security-sensitive-surface.mdc)（维护者）。

## 项目结构

源码模块与调用关系见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)（含 Mermaid 图与 `src/` 索引）。**消息同步管道**在 `GET /status` 中的计数字段及 `RUST_LOG` 排障，见该文档 **架构设计** 小节「**上下文管道（观测）**」。

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
- **MiniMax**：[开放平台 / 文档中心](https://platform.minimaxi.com)
- **智谱 GLM**：[开放平台](https://open.bigmodel.cn/) · [GLM-5 使用指南](https://docs.bigmodel.cn/cn/guide/models/text/glm-5)
- **Moonshot Kimi**：[Kimi API / Chat](https://platform.moonshot.cn/docs/api/chat)

