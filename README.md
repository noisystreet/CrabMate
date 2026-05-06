**语言 / Languages:** 中文（本页）· [English](README-en.md)

# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate Logo" width="240" />
</p>

**CrabMate** 是基于 Rust 编写的 AI Agent，通过 **OpenAI 兼容** 的 `chat/completions` 对接 DeepSeek、MiniMax、智谱 GLM、Moonshot Kimi、本地 Ollama 等后端大模型。

内置 **Function Calling** 与工作区内的命令、文件等工具，并提供 **Web UI** 与 **CLI**。

## 目录

- [功能概览](#功能概览)
- [文档索引](#文档索引)
- [后端模型支持](#后端模型支持)
- [环境与快速开始](#环境与快速开始)
- [源码编译与打包](#源码编译与打包)
- [部署与安全](#部署与安全)
- [项目结构](#项目结构)
- [参考](#参考)

## 功能概览

- **对话与多模型**：OpenAI 兼容 `chat/completions`；网关与模型见配置及下文「后端模型支持」。

- **内置工具**（**Function Calling**）：文件与工作区、**`run_command`**（白名单）、HTTP/搜索、**`codebase_semantic_search`**（SQLite **FTS5** + 本地向量，默认混合检索）、格式化、依赖图与覆盖率、容器封装等；覆盖 **Rust / Python / JS·TS / Go / JVM / C·C++** 等栈及 **GitHub `gh_*`**。全表与 JSON 示例：[docs/工具说明.md](docs/工具说明.md)。

- **CLI**：**`crabmate repl`** / **`chat`** / **`serve`**（与 Web 共用 Agent/工具）；**`tui`** 为实验性全屏终端 UI（阶段 B/C：分区骨架 + 最小对话闭环，与 **`repl`** 共用 Agent 编排；**不写 stdout 刷流式正文**，遵循 **`--no-stream`**；支持 **`/api-key`**；须交互式 TTY）。详 **[CLI](#cli)**、[docs/命令行与路由.md](docs/命令行与路由.md)。**`bench`** 批量测评；HumanEval 官方 JSONL 转换与 **`benchmark_results.jsonl`** 外挂判分见 [docs/基准测试规划.md](docs/基准测试规划.md) §5 与 **`scripts/humaneval_*.py`**（判分会执行模型生成代码，须在隔离环境运行）。

- **Web UI**：类 DeepSeek 布局；助手 **Markdown**；侧栏会话（会话项或列表空白处**右键**：**管理会话…**、收藏/取消收藏、置顶/取消置顶；列表按 **置顶 → 收藏 → 最近活动时间** 排序；导出、筛选、搜索；**桌面端**品牌行右侧可**收起/展开**左侧会话栏，偏好写入浏览器 **`localStorage`**）、工作区树与变更预览、任务与上下文状态、重试/分支；**须在侧栏「工作区」显式选择或提交目录后**，内置工具与 **`@相对路径`** 文件引用才针对该根生效（启动时不再把进程当前目录当作已选工作区；**`run_command_working_dir`** 仍用于进程合法工作目录与健康检查等，与「已选 Web 工作区」分离）。**刷新/重载**后，若会话已绑定服务端 **`conversation_id`**（`localStorage` 持久化）且配置了 **`conversation_store_sqlite_path`**，前端会 **`GET /conversation/messages`** 拉取服务端消息与 **`revision`**，与分支截断等逻辑对齐。聊天列顶部可展开 **「规划 / 工具时间线」**，汇总 **`staged_plan_step_*`** 旁注与工具摘要卡片并**一键跳转到对应气泡**（失败步骤/工具以危险色高亮）。**调试台**：右侧工具栏 **「视图」** 菜单中选 **「调试台」**，在与工作区/任务相同的右列窗格内查看 **`thinking_trace`** SSE（推理增量、`answer_phase`、工具执行前后上下文摘要等）；仅当环境变量 **`CM_THINKING_TRACE_ENABLED=0`** 时服务端关闭下发。聊天框可写 **`@相对路径`** 引用工作区文件（发送时由服务端按 **`read_file`** 规则展开），或在工作区树中**双击文件**插入 **`@路径`**；输入区可 **附加图片**（先 `POST /upload`，再在 `POST /chat/stream` 中带 `image_urls` 组装 OpenAI 兼容多模态 `user` 消息；**须使用支持视觉的模型**）。**澄清问卷**：模型可调用内置工具 **`present_clarification_questionnaire`**，Web 在 SSE 收到 **`clarification_questionnaire`** 后弹出表单，提交时随 **`POST /chat`** / **`POST /chat/stream`** 附带 JSON **`clarify_questionnaire_answers`**（`questionnaire_id` + `answers` 对象）；详见 [docs/SSE协议.md](docs/SSE协议.md) 与 [docs/工具说明.md](docs/工具说明.md)。**多角色**等见 [docs/配置说明.md](docs/配置说明.md)。

- **项目画像**：侧栏摘要与可选首轮注入；模型可用 **`repo_overview_sweep`**（[docs/工具说明.md](docs/工具说明.md)）。

- **活文档与长期记忆**：可在工作区 **`.crabmate/living_docs/`** 维护模块地图、常见坑、构建命令等 Markdown，首轮可选注入短摘要；长期记忆支持 TTL、显式 **`long_term_remember` / `long_term_forget`** 等（见 [docs/配置说明.md](docs/配置说明.md)、[docs/工具说明.md](docs/工具说明.md)）。

- **OpenAPI**：**`GET /openapi.json`**；流式控制面以 [docs/SSE协议.md](docs/SSE协议.md) 为准（含 **`client_sse_protocol`** 协商）。

- **流式与审批**：Web **SSE** + **`POST /chat/approval`**；CLI 终端审批；取消码等与 [docs/SSE协议.md](docs/SSE协议.md)、[docs/命令行与路由.md](docs/命令行与路由.md)「CLI 与 Web 能力对照」。

- **会话与导出**：Web 可选 SQLite 持久化、导出 JSON/MD（JSON 顶层与 CLI **`save-session`** 同形，含 **`schema`** / **`schema_version`** 与 **`version`**，见 [docs/命令行与路由.md](docs/命令行与路由.md) **`save-session`**）；CLI **`save-session`** / **`tool-replay`** 等。工作区变更注入、长期记忆等见 [docs/配置说明.md](docs/配置说明.md)。

- **可选**：进程内工具统计（**`agent_tool_stats_enabled`**）；工作区 **`plugins/*.json`** 动态工具（名称前缀 **`dyn__`**，运行时加载）；**MCP stdio 客户端**（**`mcp_enabled`** + **`mcp_command`**，`crabmate mcp list`）；**MCP stdio 服务端**（**`crabmate mcp serve`**，将内置工具暴露给外部 MCP 客户端，无传输鉴权）。见 [docs/配置说明.md](docs/配置说明.md)、[docs/命令行与路由.md](docs/命令行与路由.md)。

## 文档索引

| 文档 | 内容 | English |
| --- | --- | --- |
| [docs/基准测试规划.md](docs/基准测试规划.md) | `crabmate bench` 能力规划、开源 benchmark 对接与测试策略（与通用功能文档分离） | — |
| [docs/评测任务集设计.md](docs/评测任务集设计.md) | 评测任务集：覆盖矩阵、元数据、证据采集、CI 分层与安全（与 bench 契约衔接） | [en](docs/en/BENCHMARK_TASK_SUITE_DESIGN.md) |
| [benchmark/README.md](benchmark/README.md) | benchmark 快速执行指南（HumanEval 转换、执行、判分与冒烟） | — |
| [docs/分阶段规划单步设计.md](docs/分阶段规划单步设计.md) | 分阶段规划单步化设计稿：Planner 每轮 1 条 `steps` + 步后 replan，向「单智能体 + 工具循环」收敛（未实现） | — |
| [docs/中英文文档对照.md](docs/中英文文档对照.md) | 中文主文档与 `docs/en/` 英文文档的一一对应表 | — |
| [docs/开发文档.md](docs/开发文档.md) | 架构、模块索引、协议与扩展点 | [en](docs/en/DEVELOPMENT.md) |
| [docs/后端核心框架设计.md](docs/后端核心框架设计.md) | 后端核心库化与跨语言嵌入差距分析 | [en](docs/en/BACKEND_CORE_FRAMEWORK_DESIGN.md) |
| [docs/形式化验证计划.md](docs/形式化验证计划.md) | 形式化验证方案设计：SSE 不变量、属性测试、模型检查分阶段落地 | [en](docs/en/FORMAL_VERIFICATION_PLAN.md) |
| [docs/frontend/ARCHITECTURE.md](docs/frontend/ARCHITECTURE.md) | Web 前端目标架构与分阶段重构（Leptos / WASM） | — |
| [docs/工作流编排架构.md](docs/工作流编排架构.md) | 工作流编排扩展设计：状态机、条件与有界循环 | [en](docs/en/WORKFLOW_ORCHESTRATION_ARCHITECTURE.md) |
| [docs/规划执行验证架构.md](docs/规划执行验证架构.md) | 结构化规划—执行—验证闭环设计与步级验收闸门 | [en](docs/en/PLAN_EXECUTE_VERIFY_ARCHITECTURE.md) |
| [docs/测试指南.md](docs/测试指南.md) | 前后端测试、E2E、pre-commit、依赖审计命令汇总 | [en](docs/en/TESTING.md) |
| [docs/工具说明.md](docs/工具说明.md) | 内置工具说明与调用示例 | [en](docs/en/TOOLS.md) |
| [docs/SSE协议.md](docs/SSE协议.md) | `/chat/stream` 控制面 JSON | [en](docs/en/SSE_PROTOCOL.md) |
| [docs/配置说明.md](docs/配置说明.md) | 环境变量、`CM_*`、规划/上下文等配置详解 | [en](docs/en/CONFIGURATION.md) |
| [docs/调试指南.md](docs/调试指南.md) | 调试与排障：`CM_WEB_DISABLE_MARKDOWN`、`RUST_LOG`、`doctor`、SSE 对齐等 | [en](docs/en/DEBUG.md) |
| [docs/命令行与路由.md](docs/命令行与路由.md) | 子命令、选项、HTTP 路由、打包 deb | [en](docs/en/CLI.md) |
| [docs/命令行契约.md](docs/命令行契约.md) | `chat` 退出码、`--output json` 行协议、与 SSE 错误码交叉引用 | [en](docs/en/CLI_CONTRACT.md) |
| [docs/待办清单.md](docs/待办清单.md) | 未完成待办：全局 P0–P5 + 按模块分章（完成后从清单删除） | [en](docs/en/TODOLIST.md) |
| [docs/未来规划功能.md](docs/未来规划功能.md) | 方向性能力边界（如 Web 身份在网关、不进进程内账号） | [en](docs/en/FUTURE_PLANS.md) |
| [docs/代码库索引方案.md](docs/代码库索引方案.md) | 统一代码索引与增量缓存规划 | [en](docs/en/CODEBASE_INDEX_PLAN.md) |

**维护约定**：用户可见变更需同步 README 与相关文档，细则见 [docs/开发文档.md](docs/开发文档.md)「TODOLIST 与功能文档约定」。

## 后端模型支持

`POST {api_base}/chat/completions`（OpenAI 兼容）。`[agent]` 里配置 **`api_base`**、**`model`**、**`llm_http_auth_mode`**；**`bearer`** 时 **`API_KEY`** 走环境变量，**勿**写入仓库配置。

| 场景 | 配置要点 |
| --- | --- |
| **DeepSeek** | `api_base`：`https://api.deepseek.com/v1`；`model` 如 `deepseek-chat` / `deepseek-reasoner`。以 [官网](https://platform.deepseek.com/) 与 [API 文档](https://api-docs.deepseek.com/api/create-chat-completion) 为准。 |
| **MiniMax** | `api_base`：`https://api.minimaxi.com/v1`；`model` 如 `MiniMax-M2.7` 等。system 角色合并、`llm_reasoning_split` 默认等见 [CONFIGURATION「MiniMax」](docs/配置说明.md) 与 [厂商 OpenAI 兼容说明](https://platform.minimaxi.com/docs/api-reference/text-openai-api)。 |
| **智谱 GLM** | `api_base`：`https://open.bigmodel.cn/api/paas/v4`；`model` 如 `glm-5`。可选 `llm_bigmodel_thinking`。详 [CONFIGURATION](docs/配置说明.md)、[GLM-5](https://docs.bigmodel.cn/cn/guide/models/text/glm-5)。 |
| **Moonshot Kimi** | `api_base`：`https://api.moonshot.cn/v1`；`model` 如 `kimi-k2.5`。temperature 钳制、`llm_kimi_thinking_disabled` 等见 [CONFIGURATION](docs/配置说明.md)、[Kimi API](https://platform.moonshot.cn/docs/api/chat)。 |
| **本地 Ollama 等** | `llm_http_auth_mode = "none"`，`api_base` 如 `http://127.0.0.1:11434/v1`；可不设 `API_KEY`。 |

本机诊断：**`crabmate doctor`**（无需 `API_KEY`）、**`probe`** / **`models`**。完整 **`CM_*`** 与热重载见 [docs/配置说明.md](docs/配置说明.md)。**厂商能力以供应商 API 文档为准**。

## 环境与快速开始

- **Rust**：1.85+（edition 2024，见 [AGENTS.md](AGENTS.md)）

- **Docker 开发环境**（可选）：仓库 [Dockerfile](Dockerfile)（Ubuntu 24.04，Rust + trunk 等）。`docker build -t crabmate-dev .` 后 `docker run --rm -it -v "$(pwd)":/workspace -w /workspace crabmate-dev`；UID/GID 可用 `--build-arg DEV_UID` / `DEV_GID`。**未**预装 pre-commit / Node。

- **环境变量**：**`API_KEY`**（bearer 时；`serve` / `repl` / `chat` 可先启动，对话前在 Web「设置」或 REPL **`/api-key set`**）；**`models` / `probe`** 在 bearer 下通常仍需环境变量里的 Key。**`CM_API_BASE`** / **`CM_MODEL`** 覆盖配置。skills 注入可用 **`CM_SKILLS_TOP_K`** 控制每轮按用户输入选取的 Top-K 数量（默认 3）。分阶段规划：**`CM_STAGED_PLAN_FEEDBACK_MODE`**（`fail_fast` / `patch_planner`，嵌入默认见 **`config/planning.toml`**）、**`CM_STAGED_PLAN_PATCH_MAX_ATTEMPTS`**（补丁规划轮上限）；另可选 **`CM_STAGED_PLAN_TWO_PHASE_NL_DISPLAY`**（或 TOML **`staged_plan_two_phase_nl_display`**）：首轮 JSON 定稿不向用户侧流式输出，再追加一轮自然语言（默认关闭）；Web 侧可按 SSE **`staged_plan_step_*`** 在聊天区插入浅色「时间线」系统旁注（含可选 **`executor_kind`**，**不**进入模型上下文）。完整表见 [docs/配置说明.md](docs/配置说明.md#分阶段规划staged_plan_execution)。

```bash
# 可选：export CM_API_BASE=… CM_MODEL=… API_KEY=…（或 Web「设置」/ REPL /api-key set）
cargo build
./target/debug/crabmate repl    # 安装到 PATH 后可直接 crabmate repl
cd frontend && trunk build && cd ..
./target/debug/crabmate serve   # 默认 8080；发布前端用 trunk build --release
```

### CLI

- **`crabmate repl`**：交互式对话；**`/`** 内建命令与可选 **`bash#:`** 见 [docs/命令行与路由.md](docs/命令行与路由.md)。启动后可 **`/api-key set`**、**`/model set`**、**`/api-base set`**（均仅本进程内存）；bearer 无密钥时提示 **`/api-key`**。
- **`crabmate chat`**：单次非交互；**`serve`**：HTTP + Web UI（与 Web 共用逻辑）。
- **`crabmate tui`**：实验性全屏界面；**撰写区 Enter** 发送；右栏「工作区」聚焦时 **Enter** 打开工作区浏览/路径输入（左栏为会话摘要）（与 Web **`POST /workspace`**、REPL **`/workspace`** 同源校验）；**`/api-key`** 与 **`repl`** 同源（反馈写在会话区）；其它 **`/`** 命令请用 **`repl`**；输入为空时 **`q`** / **Ctrl+C** 退出；**`--no-stream`** 控制是否 SSE；模型调用 **`present_clarification_questionnaire`** 时弹出澄清问卷（**Tab** 切换题目、**Enter** 提交），答案并入下一轮用户消息（与 Web **`clarify_questionnaire_answers`** 对齐）；聊天区**不**展示系统提示词及与 Web 会话快照同类注入（仍送模型）；详见 [docs/命令行与路由.md](docs/命令行与路由.md)。
- **常用**：**`doctor`**、**`config`**、**`probe`** / **`models`**、**`bench`**、**`save-session`** / **`export-session`**、**`tool-replay`**、**`mcp list`** / **`mcp serve`**。全局选项 **`--config`**、**`--workspace`**、**`--agent-role`**、**`--no-tools`**、**`--llm-context-tokens`**、**`--no-stream`** 等。
- 配置键：[docs/配置说明.md](docs/配置说明.md)；子命令全表、Benchmark、**`man crabmate`**：[docs/命令行与路由.md](docs/命令行与路由.md)。

**前端**：`cd frontend && trunk build`（开发；**`--release`** 用于发布），再 **`crabmate serve`**。界面语言在「设置」；详 `frontend/README.md`、`docs/开发文档.md`。

**配置**：默认 `config/*.toml`（编译嵌入）+ 可选根目录 **`config.toml`**；**`system_prompt_file`** 指向 `config/prompts/default_system_prompt.md`（改后不必重编）。**`[agent] llm_context_tokens`**（或 **`CM_LLM_CONTEXT_TOKENS`**、CLI **`--llm-context-tokens`**）为模型上下文窗口 token 上限（输入+输出），与会话同步裁剪的近似字符预算推导一致（与 **`context_char_budget`** 取更小；详见 [CONFIGURATION](docs/配置说明.md)「上下文与工具消息」）。默认在首条 `system` 末附思考纪律附录（**`config/prompts/thinking_avoid_echo_appendix.md`** 等，见 [CONFIGURATION](docs/配置说明.md)）。意图增强可用 `[agent] intent_mode_bias_enabled`（默认 true）控制“意图到执行模式”的偏置；`intent_execute_low_threshold` / `intent_execute_high_threshold` 控制全局执行意图阈值，`intent_non_hier_execute_low_threshold` / `intent_non_hier_execute_high_threshold` 可单独覆盖**非分层**路径阈值；`[agent] intent_l2_enabled` 默认 **true**（L2 语义分类，额外一次无工具 `chat`；失败自动回退 L1，受 `intent_l2_min_confidence` 控制覆盖阈值）；若需节省调用可设为 **false**。**非** `Hierarchical` 时可选 `intent_at_turn_start_enabled`（`CM_INTENT_AT_TURN_START_ENABLED`）在进主循环前做一轮 L0/L1/可选 L2 门控；分层模式内建同一套管线。高级项同页。**release / deb / man** 见 **[源码编译与打包](#源码编译与打包)**。

**切换模型 / 网关**（DeepSeek、MiniMax、Ollama 等）：见上文 **[「后端模型支持」](#后端模型支持)**。

## 源码编译与打包

- **工具链**：**Rust 1.85+**、**Trunk** + **`wasm32-unknown-unknown`**；Linux / 长期记忆等见 [AGENTS.md](AGENTS.md)。
- **构建**：`cargo build` → `target/debug/crabmate`；**`--release`** → `target/release/crabmate`。带 Web 时先 **`cd frontend && trunk build`**（发布用 **`--release`**）。
- **可选 Cargo features**（根包 **`crabmate`**，默认 **`mcp` + `docker_sandbox` + `fastembed`**，与完整产品一致）：裁剪 **`rmcp`**（MCP）、**`bollard`**（Docker 沙盒）或 **`fastembed`**（本地向量嵌入 / ONNX）时用 **`cargo build --no-default-features`** 或按需 **`--features mcp,docker_sandbox`** 等；关闭 **`docker_sandbox`** 时**勿**在配置里使用 **`sync_default_tool_sandbox_mode = docker`**；关闭 **`fastembed`** 时 **`codebase_semantic_search`** 会从工具列表中移除，**`hybrid`** 查询退化为 **FTS**，**`finalize`** 会将 **`long_term_memory_vector_backend=fastembed`** 自动降为 **`disabled`**（仍保留长期记忆 SQLite，仅无向量）。维护者说明见 **`docs/开发文档.md`** 与 **`docs/后端核心框架设计.md`**。
- **检查**：`cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`；或 [.pre-commit-config.yaml](.pre-commit-config.yaml)。完整测试与质量检查命令见 **[docs/测试指南.md](docs/测试指南.md)**。
- **SSE 协议回归**：可执行 **`./scripts/check-sse-protocol.sh`** 一键跑协议属性测试 + 控制面分类金样/顺序校验（若网络受限，先设置 `http_proxy`/`https_proxy`）。
- **E2E**（可选）：`frontend` 构建后 **`cd e2e && npm ci && npx playwright install chromium && npm test`**。见 [docs/测试指南.md](docs/测试指南.md) 与 [docs/开发文档.md](docs/开发文档.md)。
- **安装**：`cargo install --path .`（**不**自动装 man；`.deb` 包或手动 [man/crabmate.1](man/crabmate.1)）。同步 clap 与 troff：`cargo run --bin crabmate-gen-man`。
- **一键打包**：**`./scripts/package-release.sh`** → **`dist/`** 下 **`crabmate_<version>_<os>_<arch>.tar.gz`**（含二进制、`config/`、`frontend/dist`、man）；Linux 且已安装 **`cargo-deb`** 时同时复制 **`target/debian/crabmate_*.deb`** 到 **`dist/`**。
- **`.deb`**：[cargo-deb](https://github.com/kornelski/cargo-deb)，亦可手动：前端 release + **`cargo deb`**，默认产物 **`target/debian/`**。详 [docs/命令行与路由.md](docs/命令行与路由.md)「打包 Debian `.deb`」。

## 部署与安全

- **监听**：默认 **`127.0.0.1`**；`0.0.0.0` 须 **`web_api_bearer_token`** 或显式不安全开关（见 [docs/配置说明.md](docs/配置说明.md)）。
- **Web API 密钥**：嵌入默认 **`web_api_require_bearer = true`**：启动 **`serve`** 前须在环境中设置 **`CM_WEB_API_BEARER_TOKEN`**（或 TOML **`web_api_bearer_token`**），否则进程拒绝启动。请求头 **`Authorization: Bearer …`** 或 **`X-API-Key: …`**（二选一）。前端可读 **`localStorage["crabmate-api-bearer-token"]`**。纯本地匿名调试可在配置中设 **`web_api_require_bearer = false`** 且不设密钥（仍仅限可信环境）。
- **调试与排障**：环境变量、日志、`doctor`、HTTP 探针、SSE 对齐等见 **[docs/调试指南.md](docs/调试指南.md)**（含 **`CM_WEB_DISABLE_MARKDOWN`**、**`CM_WEB_RAW_ASSISTANT_OUTPUT`** 与 **`GET /web-ui`**）；变量说明亦在 [docs/配置说明.md](docs/配置说明.md)「Web 服务」。可选 **`CM_REPLAY_DUMP_DIR`** 在对话执行期间**即时追加** `turn-replay-events.jsonl`（动作级事件流），详见 [docs/配置说明.md](docs/配置说明.md)「整请求 Chrome Trace」表。
- **Web「设置」**：界面语言 / 主题 / 背景与本机 **`client_llm`**（`api_base` / `model` / `temperature` / **`llm_context_tokens`**（模型上下文 token 上限）/ **`llm_thinking_mode`**（`server` / `on` / `off`，控制每轮发往模型的 thinking 相关字段）/ 密钥）等修改后，需点击 **「保存全部」** 才写入浏览器 **`localStorage`** 并随请求生效；`api_base` 可从常用供应商预设中选择或手写，`temperature` 支持 `0~2`（留空使用服务端默认），详 [docs/配置说明.md](docs/配置说明.md)「Web 对话队列」。
- **工作区**：须在允许根内；Unix 上尽力用 **`openat2`** 等收窄路径风险，**非**绝对沙箱。见 [docs/配置说明.md](docs/配置说明.md)、[`src/workspace/path.rs`](src/workspace/path.rs)。
- **其它**：**`web_search_api_key`** 与主 **`API_KEY`** 分离；可选 **SyncDefault Docker 沙盒**见 [docs/配置说明.md](docs/配置说明.md)。维护者另见 [docs/开发文档.md](docs/开发文档.md)、[.cursor/rules/security-sensitive-surface.mdc](.cursor/rules/security-sensitive-surface.mdc)。

## 项目结构

模块与调用链、**`GET /status` 观测**、**`src/`** 索引见 [docs/开发文档.md](docs/开发文档.md)。

- **Workspace 成员**：`crates/crabmate-sse-protocol`（SSE 控制面版本与分类契约）；**`crates/crabmate-im-bridge`**（可选 **IM 桥接**二进制：飞书事件 Webhook → CrabMate **`POST /chat`** → 飞书回复）。飞书接入步骤与限制见 [docs/design/feishu_bridge_mvp.md](docs/design/feishu_bridge_mvp.md)（与 [docs/design/web_api_integration.md](docs/design/web_api_integration.md) 配套）。

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
- **MiniMax**：[开放平台 / 文档中心](https://platform.minimaxi.com)
- **智谱 GLM**：[开放平台](https://open.bigmodel.cn/) · [GLM-5 使用指南](https://docs.bigmodel.cn/cn/guide/models/text/glm-5)
- **Moonshot Kimi**：[Kimi API / Chat](https://platform.moonshot.cn/docs/api/chat)
