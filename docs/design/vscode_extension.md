# 官方 VS Code 扩展：设计备忘与路线图

**状态**：设计备忘 / 路线图（**未**承诺实现顺序与版本日期）。**受众**：计划实现或评审「CrabMate × VS Code」官方扩展的维护者；亦供产品与安全评审引用。  
**语言**：中文。  
**存放位置**：按仓库惯例置于 **`docs/design/`**（与 **`web_api_integration.md`**、**`tauri_gui_mvp_design.md`** 同级）；**不**单独要求 `RFC/` 目录；若日后 RFC 流程成型，可再迁移至 **`docs/rfc/`** 并保留本路径重定向说明。

**关联**：

- HTTP / SSE 契约：**`docs/命令行与路由.md`**、**`docs/SSE协议.md`**、**`crates/crabmate-sse-protocol`**
- Web 鉴权与健康：**`README.md`**、**`docs/配置说明.md`**（`CM_WEB_API_BEARER_TOKEN`、`web_api_require_bearer`、非 loopback 等）
- 第三方 HTTP 集成总览：**`docs/design/web_api_integration.md`**
- MCP（与扩展**并列**的能力面）：**`docs/开发文档.md`**（`mcp/mod.rs`）、**`docs/命令行与路由.md`**（`mcp serve` / `mcp list`）
- 密钥与日志：**`.cursor/rules/secrets-and-logging.mdc`**
- 工作区安全叙事：**`README.md`**、**`docs/配置说明.md`**（工作区路径、`openat2` 等）

---

## 1. 目标与边界

### 1.1 本文讨论什么

- 在 **Visual Studio Code**（及兼容市场如 **Open VSX / VSCodium** 若选择支持）中，以**官方扩展**形态提供：连接本机或可配置的 **CrabMate `serve`**、流式对话、工作区绑定、鉴权、（后续）审批/澄清等与 Web **协议对齐**的能力。
- 扩展作为 **「编辑器壳 + HTTP/SSE 客户端」**：推理与工具执行仍在 CrabMate 进程内，**不**在扩展宿主内嵌 `run_agent_turn`。

### 1.2 本文不讨论什么

- **Leptos Web UI** 的逐屏复刻；扩展内 UI 以 **Webview 自绘**或受控加载为主（见第 2 节）。
- **MCP 协议在扩展内的重实现**：MCP 继续由 **`crabmate mcp serve`** / 配置 **`mcp_command`** 等路径承担；扩展可与 MCP **并存**，职责不重叠。
- **工作区 `plugins/*.json` 动态工具**的解析逻辑：仍由服务端 **`run_agent_turn`** 扫描；扩展只须保证工作区根与 **`POST /workspace`** 语义一致。

---

## 2. 推荐架构

```
VS Code Extension Host
  ├─ 配置与密钥：settings + SecretStorage
  ├─ Sidebar：会话列表 / 连接状态（可选）
  ├─ Webview：聊天主界面（Markdown、代码块、控制面事件映射）
  └─ HTTP 客户端：fetch + SSE（与 Web 对齐的控制面解析与错误码）
           │
           ▼
   crabmate serve（默认 http://127.0.0.1:<port>）或用户配置的 apiBase
```

**原则**：

1. **协议复用**：与 **`POST /chat`、`POST /chat/stream`**、**`docs/SSE协议.md`** 一致；扩展侧宜有 **TypeScript 单测**或共享生成类型，与 **`fixtures/sse_control_golden.jsonl`** / **`crabmate-sse-protocol`** 语义对齐（实现方式可选：手写解析、从 OpenAPI 生成、或子仓 CI 对照金测向量）。
2. **薄壳**：扩展不写第二套「工具执行」与「工作区沙箱」；一律调用现有 HTTP API。
3. **密钥**：**Web API Bearer** 等敏感项用 **`context.secrets`**，**不**写入用户可同步的 `settings.json`；日志遵循仓库脱敏规则。

**可选演进**（权衡后再选）：

- **Webview 内嵌 `serve` 已有页面**：可减少 UI 重复，但需处理 **CSP、iframe、postMessage** 与 VS Code 安全模型；首版通常 **自绘 Webview** 更可控。

---

## 3. 与后端的连接与鉴权

| 项 | 建议 |
|----|------|
| **Base URL** | 配置项如 `crabmate.apiBase`；默认 `http://127.0.0.1:8080`（以实际默认为准）。 |
| **可达性** | 启动前或面板打开时 **`GET /health`**；失败时给出「是否已 `cargo run -- serve`」类文案。 |
| **Bearer** | 与 Web 一致：非 loopback 等场景须 **`CM_WEB_API_BEARER_TOKEN`** 或等价配置；扩展从 **SecretStorage** 注入请求头。 |
| **工作区** | 使用 **`workspace.workspaceFolders`** 与用户显式选择，调用现有 **`POST /workspace`**（或文档规定的等价路径），**避免**扩展侧自行 `canonicalize` 绕过服务端策略。 |

**云厂商 API Key**：首版可约定「仅由 CrabMate 进程通过环境变量 / 配置读取」；若需与 Web 一致支持请求体 **`client_llm`**，须在 UI 与文档中明确 **不落盘明文**、并遵守脱敏规则。

---

## 4. 功能分期（建议）

### MVP

- 配置：`apiBase`、Bearer、可选「检测 `serve`」。
- 单会话：**流式**聊天、`AbortSignal` 取消、与 **`docs/SSE协议.md`** 对齐的错误展示。
- 工作区绑定与切换（与 API 一致）。

### v1

- 多会话列表 / 切换（依赖现有 **`GET /conversation/messages`** 等只读接口或后续 API）。
- **工具审批 / 澄清问卷**：用 **Modal / QuickPick** 映射 SSE 控制面 + **`POST /chat/approval`** 等（与 **`docs/design/web_api_integration.md`** 审批约束一致）。

### v2

- **断线重连**：`Last-Event-ID`、**`stream_resume`** 与 **`x-stream-job-id`** 与 Web 行为对齐；处理 **410 / `STREAM_JOB_GONE`** 等（见 **`web_api_integration.md`** 对 SSE 的约束说明）。
- **主题与无障碍**：跟随 VS Code **ColorTheme**、高对比；键盘与焦点在 Webview 与编辑器间可预测。

---

## 5. 与 MCP、`plugins/*.json` 的关系

| 机制 | 扩展角色 |
|------|----------|
| **MCP** | 扩展**不替代** MCP server/client；用户可在 VS Code 内使用 **其它 MCP 扩展** + **`crabmate mcp serve`** 与聊天扩展**并行**。 |
| **`plugins/*.json`** | 扩展**不解析**；只要工作区根正确，由 CrabMate 每轮扫描注册动态工具。 |

---

## 6. 实现与交付需额外考虑的因素

### 6.1 运行环境拓扑

- **Remote SSH / WSL / Dev Container**：扩展与 **`serve` 须在同一「网络可达侧」**；`127.0.0.1` 在 Remote 内指向远程机而非本机桌面。文档须写清推荐拓扑（例如在 Remote 内同时启动 `serve`）。
- **多根工作区**：定义「当前绑定到 CrabMate 的根文件夹」规则，避免歧义。

### 6.2 版本与兼容

- **扩展 semver ↔ CrabMate 版本**：大版本 bump 时 SSE/字段变更的降级或强提示。
- 可选：依赖 **`GET /health`** 或专用 **capabilities** JSON 做特性协商（需后端配合时再定）。

### 6.3 安全与合规

- **HTTPS / 企业代理**：证书校验失败时的排障路径；尽量不绕过校验。
- **Marketplace / Open VSX**：品牌、隐私声明、遥测默认策略（建议默认关闭或显式 opt-in）。
- **权限最小化**：`package.json` **`contributes`** 仅声明必要 **`workspace`** / **`terminal`**（若提供「一键启动 serve」任务）等。

### 6.4 仓库与 CI

- **推荐独立仓库**（如 `crabmate-vscode`）：npm 周期、与 Rust 主仓发版解耦；主仓 **`docs/design/vscode_extension.md`** 保留架构与链接即可。
- **CI**：`npm test`、对 **mock SSE** 的契约测试；可选夜间对 **固定版本** CrabMate 二进制做 e2e。

### 6.5 支持成本

- **诊断导出**：扩展版本、`apiBase`、是否 loopback、**不含** token；与 **`crabmate doctor`** 文档交叉引用。

---

## 7. 文档与 RFC 目录

- **当前**：本文件即权威设计备忘；与 **`docs/design/web_api_integration.md`** 分工——后者侧重 **IM/自动化 HTTP 集成**，本文侧重 **VS Code 宿主内客户端**。
- **日后**：若引入编号 RFC 流程，可将已定稿迁入 **`docs/rfc/`** 并在本路径保留一行「已迁移至 …」；**非**强制第一步就建 `RFC/`。

---

## 8. 开放问题（供评审）

1. 首版是否**仅支持** VS Code 正式版 LTS 范围；是否声明 **不支持 Cursor 专有 API**（若支持 Cursor，需单独兼容性声明）。
2. 是否在扩展内提供 **「安装/更新 CrabMate 二进制」** 引导，还是仅文档链接（降低供应链与签名责任）。
3. **`client_llm`** 是否在首版暴露给终端用户，或完全依赖服务端环境。

---

*本文随实现进展修订；落地任务若进入仓库级待办，应同步 **`docs/待办清单.md`** 并在完成后按仓库惯例删除已完成条目。*
