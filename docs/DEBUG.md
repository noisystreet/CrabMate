**语言 / Languages:** 中文（本页）· [English](en/DEBUG.md)

# 调试与排障指南

本文汇总 CrabMate 常用的**调试手段**（环境变量、日志、HTTP 探针、内置工具、协议与测试），与 [docs/CONFIGURATION.md](CONFIGURATION.md)（配置与 `AGENT_*` 全表）、[docs/CLI.md](CLI.md)（子命令与路由）、[docs/DEVELOPMENT.md](DEVELOPMENT.md)（架构与可观测性细节）互补。

---

## 1. Web UI：CSR 展示相关环境变量

以下变量均**无**对应 TOML 字段；设为真值（**`1`** / **`true`** / **`yes`** / **`on`**，大小写不敏感）即生效；修改后须**重启 `serve`**。浏览器 CSR 启动后会请求 **`GET /web-ui`**；若请求失败，前端默认仍**开启** Markdown（`markdown_render` 视为真）并**开启**助手展示过滤（`apply_assistant_display_filters` 视为真）。

| 环境变量 | 响应 JSON 字段 | 效果 |
| --- | --- | --- |
| **`AGENT_WEB_DISABLE_MARKDOWN`** | **`markdown_render`** 为 `false` | **助手气泡**与**工作区变更集模态**以 **HTML 转义纯文本**展示（换行转为 `<br />`）。**聊天气泡内**思维链与终答均为同一正文色与等宽字体，**不再**使用 Markdown 模式下的次要色/左边线/背景卡区分思维链 |
| **`AGENT_WEB_RAW_ASSISTANT_OUTPUT`** | **`apply_assistant_display_filters`** 为 `false` | **不对助手返回内容做 UI 侧改写**：不剥 `agent_reply_plan` 围栏/前缀 JSON、不按内联 `</redacted_thinking>` 等标记拆分思维链与正文；与侧栏跨会话搜索、会话内查找、复制、浏览器内 JSON/Markdown 导出使用同一套展示文本。**未**开启本变量时（默认），分阶段规划下**无工具规划轮**经 SSE 到浏览器时：若解析规划 JSON 为 **`no_task: true`** 则**整轮**不落 SSE；否则**仅**丢弃该轮在 CrabMate 信封 `assistant_answer_phase` **之前**的流式增量（规划轮「思考」路径），信封与之后正文仍下发（`staged_plan_two_phase_nl_display` 开启时整轮规划仍由 NL 补全轮承担可见输出，与此前一致）。与上一行 **Markdown 开关**相互独立 |

**手动验证**：`curl -s http://127.0.0.1:8080/web-ui`（端口按实际；若启用了 Web API 鉴权，与其它系统路由一致处理）。

---

## 2. 服务端日志（`RUST_LOG` 与 `--log`）

- **实现**：进程使用 **`tracing`**（**`tracing-subscriber`**）输出；既有 **`log::`** 调用经 **`tracing-log`** 进入同一套 subscriber。**`RUST_LOG`** 语法与 **`env_logger`** 时代相同（见 [tracing-subscriber `EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)）。
- **JSON 行**（便于 `jq` / 日志平台）：设 **`AGENT_LOG_JSON=1`**（或 **`true`** / **`yes`** / **`on`**）；与 **`--log`** 组合时 stderr 与文件均为 JSON。
- **Web 排障关联**：流式任务日志可带根 span **`chat_turn`** 的字段 **`job_id`**、**`conversation_id`**（截断预览 + **`conversation_id_len`**）、**`outer_loop_iteration`**、**`tool_call_id`**（截断预览；与 HTTP **`x-stream-job-id`** / SSE **`sse_capabilities.job_id`** 对齐的单调 id 为 **`job_id`**）。需要完整会话 id 或更长请求体预览时请查会话存储或开 **`RUST_LOG=crabmate=debug`**（见 **`AGENT_LOG_CHAT_REQUEST_JSON`** 与 [`src/redact.rs`](../src/redact.rs)）。
- **默认**：未设置 `RUST_LOG` 时，`serve` 为 **info**；`repl` / `chat` / `bench` / `config` / `save-session` / `tool-replay` 等为 **warn**。见 [docs/CLI.md](CLI.md)。
- **全局文件 + 镜像 stderr**：根级 **`--log /path/to.log`**（须写在子命令**之前**，如 `crabmate --log /tmp/cm.log serve`）。
- **上下文管道（每轮进模型前）**  
  - `RUST_LOG=crabmate=debug`：打印 **`message_pipeline session_sync`** 汇总一行。  
  - `RUST_LOG=crabmate::message_pipeline=trace`：每阶段一行 **`session_sync_step`**（阶段名、消息条数、字符估计等）。  
  计数与含义见 [docs/DEVELOPMENT.md](DEVELOPMENT.md)「架构设计 → 上下文管道（观测）」；与 **`GET /status`** 中相关字段对照。
- **规划 / 反思（`per`）**：`RUST_LOG=crabmate::per=info` 或更宽级别，可看 `after_final_assistant`、重写次数等（见 DEVELOPMENT）。
- **CLI 终端打印路径**：`RUST_LOG=crabmate::print=debug` 可在终端打印前对将输出内容做**截断预览**（便于对照实际气泡/工具块），见 DEVELOPMENT 中 `terminal_cli_transcript` 等说明。

**勿在日志或 issue 中粘贴**完整 **`API_KEY`**、Bearer 头或带真实 token 的 URL；见仓库 **`.cursor/rules/secrets-and-logging.mdc`**。

---

## 3. 本地诊断子命令（无需对话模型即可使用）

| 命令 | 用途 |
| --- | --- |
| **`crabmate doctor`** | 一页式本机诊断（Rust/路径/可选依赖等）；**不要**求 `API_KEY` |
| **`crabmate probe`** / **`crabmate models`** | 探测 `GET {api_base}/models`；**bearer** 模式下通常需要环境变量 **`API_KEY`**（输出脱敏） |
| **`crabmate save-session`**（别名 **`export-session`**） | 导出会话 JSON/Markdown，便于离线对照消息与工具结果 |
| **`crabmate tool-replay`** | 从会话提取工具步骤 fixture 并重放，便于隔离工具层问题 |

REPL 内等价：**`/doctor`**、**`/probe`**、**`/models`** 等，见 [docs/CLI.md](CLI.md)。

---

## 4. HTTP 探针（`serve` 运行时）

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | **`/health`** | 依赖与健康项（含可选 LLM models 探活等） |
| GET | **`/status`** | 模型、工具数、规划/队列/上下文管道计数等运行态摘要 |
| GET | **`/web-ui`** | CSR 展示开关 JSON（**`markdown_render`**、**`apply_assistant_display_filters`**，见 §1） |
| GET | **`/openapi.json`** | OpenAPI 3.0，与当前路由表对齐 |

完整路由表见 [docs/CLI.md](CLI.md)「主要 HTTP 路由」。若进程启用了 Web API 鉴权，受保护路径须带 **`Authorization: Bearer …`** 或 **`X-API-Key: …`**；**`/health`**、**`/status`**、**`/web-ui`**、**`/openapi.json`** 与静态页所在层以当前 `src/web/server.rs` 为准。

---

## 5. 内置工具 `diagnostic_summary`

模型可调用 **`diagnostic_summary`**（参数均可选）收集**只读、脱敏**信息：Rust 工具链版本、工作区常见路径是否存在、若干环境变量**是否已设置**（**永不输出变量值**；与 `API_KEY` 同类变量**亦不报告长度**）。

**不要**把真实密钥粘贴进对话或工具入参。参数与行为见 [docs/TOOLS.md](TOOLS.md)。

---

## 6. SSE 与前后端协议对齐

- **权威说明与错误码**：[docs/SSE_PROTOCOL.md](SSE_PROTOCOL.md)。
- **后端**：`src/sse/protocol.rs`、`crates/crabmate-sse-protocol`（版本号 **`SSE_PROTOCOL_VERSION`**）。
- **前端**：`frontend-leptos/src/sse_dispatch.rs`、`frontend-leptos/src/api.rs`。
- **修改控制面 JSON 分支顺序**时：同步 **`crates/crabmate-sse-protocol/control_classify.rs`**、**`frontend-leptos/src/sse_dispatch.rs`** 与 **`fixtures/sse_control_golden.jsonl`**，并执行：**`cargo test golden_sse_control`**。

---

## 7. CLI 流式与规划输出（可选环境变量）

| 变量 | 说明 |
| --- | --- |
| **`AGENT_CLI_WAIT_SPINNER=1`** | 在等待模型首包流式输出（或非流式整段 body）时，于 **TTY stderr** 显示等待动效（默认关；见 [docs/CONFIGURATION.md](CONFIGURATION.md)） |
| **`AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM=0`** | 关闭交互式 CLI 下无工具规划轮的模型原文打印（仍保留步骤摘要等） |

---

## 8. 工作流 / 请求 Chrome Trace（可选）

将工作流或 HTTP 请求轨迹导出为 Chrome Trace JSON 时，可使用环境变量 **`CRABMATE_WORKFLOW_CHROME_TRACE_DIR`** / **`AGENT_WORKFLOW_CHROME_TRACE_DIR`** 等（与 **`CRABMATE_REQUEST_CHROME_TRACE_DIR`** 的合并行为见 [docs/CONFIGURATION.md](CONFIGURATION.md) 与 [docs/DEVELOPMENT.md](DEVELOPMENT.md)）。

---

## 9. 前端（Leptos / WASM）本地构建

- 静态资源：**`cd frontend-leptos && trunk build`**（发布用 **`trunk build --release`**），再由 **`crabmate serve`** 从 **`frontend-leptos/dist`** 提供。
- 维护者快速类型检查：**`cd frontend-leptos && cargo check --target wasm32-unknown-unknown`**。
- 浏览器侧：开发者工具 **Network**（`POST /chat/stream`、`GET /web-ui` 等）、**Console**（WASM  panic 由 `console_error_panic_hook` 辅助）。

---

## 10. 相关文档索引

| 文档 | 内容 |
| --- | --- |
| [CONFIGURATION.md](CONFIGURATION.md) | `AGENT_*`、热重载、Web 鉴权与安全开关 |
| [CLI.md](CLI.md) | 子命令、HTTP 路由、`RUST_LOG` 默认 |
| [DEVELOPMENT.md](DEVELOPMENT.md) | 模块索引、`message_pipeline` 日志与 `/status` 计数 |
| [SSE_PROTOCOL.md](SSE_PROTOCOL.md) | SSE 行协议与错误码 |
| [TOOLS.md](TOOLS.md) | `diagnostic_summary` 等工具说明 |
