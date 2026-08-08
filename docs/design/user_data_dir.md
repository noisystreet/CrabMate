# 本机用户数据目录（`~/.local/share/crabmate`）设计

## 1. 背景与问题

**历史问题**（已解决）：早期 Web / Tauri 曾把用户级状态写在浏览器 **`localStorage`**（Tauri WebKit 按 `http://127.0.0.1:<port>` 分叉），导致跨端口不一致、难备份，且 CLI/TUI 无法读取。

**工作区内**数据（`conversations.db`、导出、`repl_history.txt` 等）落在 **`<workspace>/.crabmate/`**，与本设计**互补**，不合并。

**用户级 Agent 配置 TOML**（与本目录分离）：默认 **`$XDG_CONFIG_HOME/crabmate/`**（**`CM_CRABMATE_CONFIG_DIR`**）。发现顺序为 cwd 本地覆盖 → XDG；桌面 deb 系统模板在 **`/etc/crabmate/`**；用户尚无 **`config.toml`** 时首次种子拷贝运行时子集到 XDG Config（**不覆盖**；含可选 **`skills/`**；日常只读用户副本，种子失败时桌面可只读回退 `/etc`）。用户级 skills 默认目录为 **`$XDG_CONFIG_HOME/crabmate/skills`**（与工作区 **`.crabmate/skills`**、系统 **`/etc/crabmate/skills`** 合并；与是否自动加载 XDG `config.toml` 无关，见 **`docs/配置说明.md`**）。

**可清理缓存**：默认 **`$XDG_CACHE_HOME/crabmate/`**（**`CM_CRABMATE_CACHE_DIR`**）；含 **`fastembed/`** ONNX 模型。**不要**把会话/密钥放进 cache。

**状态**：**已实现（P0–P4）**；Web 经 **`/user-data/*`** 读写 **`$XDG_DATA_HOME/crabmate`**（默认 **`~/.local/share/crabmate`**）。

---

## 2. 目标与非目标

### 2.1 目标

| 目标 | 说明 |
|------|------|
| **单一真源** | 用户级 UI 状态与 LLM 本机覆盖集中至 **`~/.local/share/crabmate`**（可配置根目录） |
| **三端共用** | Web、Tauri（经本机 `serve`）、CLI/TUI（Rust 直读或同一 HTTP API）共用同一套文件 |
| **可迁移** | （已移除）旧版 `localStorage` 导入；新装直接使用磁盘目录 |
| **安全分级** | 非机密进 JSON；API Key 等进 **`secrets/`** 且 HTTP 不回传明文 |

### 2.2 非目标（本阶段不替代）

- **不**替代 `<workspace>/.crabmate/conversations.db`（服务端对话持久化，`conversation_id` 消息链）；
- **不**替代 REPL **`repl_history.txt`**（按工作区）；
- **不**将大段对话正文迁入 home（仅侧栏会话元数据、草稿、绑定 id）；
- **不**使用 **`~/.cache/crabmate`** 存放需长期保留的会话与密钥（cache 语义可被系统清理）。

---

## 3. 目录布局

根目录解析（Rust 单点，三端共用）：

```text
CM_CRABMATE_USER_DATA_DIR  → 若设置且非空，使用该路径
否则 XDG_DATA_HOME/crabmate → Linux 通常为 ~/.local/share/crabmate
```

并列的 XDG 根（实现见 **`crabmate-config::xdg`** / **`user_config_xdg`**）：

| 用途 | 默认 | 覆盖 |
|------|------|------|
| 配置 | `$XDG_CONFIG_HOME/crabmate` | `CM_CRABMATE_CONFIG_DIR` |
| 缓存 | `$XDG_CACHE_HOME/crabmate`（含 `fastembed/`） | `CM_CRABMATE_CACHE_DIR` |
| 数据 | `$XDG_DATA_HOME/crabmate`（本设计） | `CM_CRABMATE_USER_DATA_DIR` |

```text
~/.local/share/crabmate/
├── meta.json                         # schema_version、migrated_from、updated_at_ms
├── prefs.json                        # 全局非机密偏好（见 §4.1）
├── llm_overrides.json                # LLM 非机密覆盖（见 §4.2）；可与 prefs 合并为一文件
├── mcp_servers.json                  # MCP stdio 多服务器（见 §4.5）；Web 设置 → MCP
├── global/
│   └── web_sessions.json             # 未设置 Web 工作区时的会话桶（现 agent-demo-sessions-v1）
├── workspaces/
│   └── <ws_sha256>/                  # SHA256(hex)，与 frontend sessions_json_storage_key 一致
│       ├── manifest.json             # workspace_root 规范绝对路径
│       └── web_sessions.json         # 侧栏 ChatSession[] + active_session_id
└── secrets/                          # 仅遗留迁移用；新写入走系统钥匙串（见 §4.6）
```

**`ws_sha256` 算法**：与 `frontend/src/storage.rs` 中 `normalize_workspace_partition_path` + SHA256 相同，便于从 `agent-demo-sessions-v1::ws::<hex>` 键名一对一迁移。

---

## 4. 文件 Schema

### 4.1 `meta.json`

```json
{
  "schema_version": 1,
  "migrated_from": ["localStorage", "tauri-webkit"],
  "updated_at_ms": 0
}
```

### 4.2 `prefs.json`（全局，非机密）

| 字段 | 历史 localStorage 键（参考） | 说明 |
|------|---------------------------|------|
| `last_workspace_root` | （计划键 `crabmate-last-workspace-root`） | 上次手动 `POST /workspace` 成功的规范路径（与 `recent_workspace_roots[0]` 同步）；供「最近的工作区」菜单；**启动时不**自动打开 |
| `recent_workspace_roots` | — | 最近打开的工作区根列表（**新在前**，最多 **10** 项）；Web/Tauri **「项目 → 最近的工作区」** 级联子菜单读取此列表 |
| `locale` | `crabmate-locale` | |
| `theme` | `crabmate-theme` | 含 **`system`**（跟随 OS；DOM 解析为 `dark`/`light`） |
| `side_panel_view` | `agent-demo-side-panel-view` | |
| `side_width` | `agent-demo-workspace-width` | |
| `editor_layout_mode` | `crabmate-editor-layout-mode` | |
| `timeline_panel_expanded` | `crabmate-timeline-panel-expanded` | |
| `sidebar_rail_collapsed` | `crabmate-sidebar-rail-collapsed` | |
| `session_ui_font` / `session_chat_font` / `session_chat_font_size` | `crabmate-session-*-font`（字号无历史键） | 会话模式 UI/聊天气泡字体族与聊天区字号（px） |
| `ide_editor_*` | `crabmate-ide-editor-*` | |

**不含**：`api_key`、完整 `web_api_bearer_token`（见 §4.4）。

### 4.3 `workspaces/<ws_sha256>/web_sessions.json`

与现 `SessionsFile` 同形（`frontend/src/storage.rs`）：

```json
{
  "schema_version": 1,
  "sessions": [],
  "active_session_id": "s_..."
}
```

每条 `ChatSession` 可含 `server_conversation_id`、`workspace_root`、草稿、`messages` 本地展示缓存等（字段保持与前端 serde 一致）。`layout_schema_version` 标识消息行投影版本：旧缓存缺省为 `1`，新建会话及 v2 流式投影写 `2`；v1 reader 可忽略该扩展字段。

### 4.4 `llm_overrides.json`（非机密 LLM 覆盖）

```json
{
  "schema_version": 1,
  "client_llm": {
    "api_base": "https://api.deepseek.com/v1",
    "model": "deepseek-chat",
    "temperature": null,
    "llm_context_tokens": null,
    "llm_thinking_mode": "server"
  },
  "executor_llm": {
    "api_base": "",
    "model": ""
  },
  "execution_mode": null,
  "saved_models": []
}
```

对应现 `frontend/src/api/client_llm_storage.rs` 中非密钥字段。

### 4.5 `mcp_servers.json`（MCP stdio 多服务器）

仅存本机 user-data（**不**写 TOML / 工作区）。`slug` 由 **`name` 自动生成**（小写字母数字与下划线；冲突时追加 `_2` 等），OpenAI 工具名为 `mcp__{slug}__{remote}`。

```json
{
  "schema_version": 1,
  "global_enabled": true,
  "tool_timeout_secs": 60,
  "servers": [
    {
      "id": "mcp_1730000000000",
      "name": "Filesystem",
      "slug": "filesystem",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"],
      "env": {},
      "cwd": null,
      "enabled": true,
      "created_at_ms": 0,
      "updated_at_ms": 0
    }
  ]
}
```

- **`command`**：可执行文件路径；若 **`args` / `env` / `cwd` 皆空**，则将 `command` 视为 legacy **整行**命令并按 shell 词法拆分（兼容旧的 `sh -c '…'` 落盘）。
- **`args` / `env` / `cwd`**：结构化启动；导入 MCP JSON 时原样写入，**不再**合成 `sh -c`。

若文件为空、**`toml_legacy_imported` 未置位**，且 TOML/`CM_MCP_COMMAND` 仍启用 legacy 单条 `mcp_command`，**一次性**导入为单服务器并置 **`toml_legacy_imported: true`**；已有非空 `servers` 时也会落该标记（清空列表后不再重导）。之后以本文件为准。HTTP：`GET/PUT /user-data/mcp-servers`（**GET 响应**不含启动明文，仅 `has_command` / `has_args` / `has_env` / `has_cwd` / `has_url` / `has_headers` / `has_bearer`）、`POST …/import`（JSON 解析并追加）、`PUT …/{id}/remote-auth`（Bearer → 系统钥匙串账户 `mcp_bearer_{id}`）、`GET …/status`（含 `transport`、连接失败时的 `last_error` / `last_error_kind`）、`POST …/{id}/probe`。

Web **设置 → MCP → 从 MCP JSON 导入**：粘贴含 **`mcpServers`** 的配置（可为整份 **`mcp.json`** 或其中一段），解析后追加到列表（`name` 取自键名；`command`/`args`/`env`/`cwd` **结构化落盘**，或仅 **`url`** 的远程条目；`slug` 仍于保存时由 `name` 生成）。含 `${env:…}` / `${workspaceFolder}` 等占位符时保留原文并提示手动改路径或环境变量。远程行可在设置页单独保存 Bearer（不经 GET 回显）。

### 4.6 系统钥匙串与遗留 `secrets/`

持久密钥写入系统钥匙串（服务名 **`com.crabmate.credentials`**；macOS Keychain / Windows Credential Manager / Linux Secret Service）。账户名：

| 账户 | 内容 |
|------|------|
| `client_llm` | 云厂商 Bearer（主模型） |
| `executor_llm` | 可选：执行器 API Key |
| `web_api_bearer` | 访问 `/chat`、`/user-data` 等的 CrabMate HTTP 鉴权（经 `/user-data/secrets` 或 CLI **`crabmate web-bearer set`**；**`serve`** 在 TOML/`CM_WEB_API_BEARER_TOKEN` 皆空时从此处回退加载；与服务端校验值一致时由前端携带） |
| `github` | GitHub user access token / PAT（经 `/user-data/secrets/github` 或 Device Flow；供子进程 `gh` 注入 `GH_TOKEN`，并对 `https://github.com/` 的 clone/push/fetch 注入 HTTPS Basic `x-access-token`） |
| `github_oauth_client_id` | GitHub App / OAuth App **Client ID**（经 `/user-data/secrets/github-oauth-client-id`；供 Device Flow；**非** Client Secret；status 仅 `set`/后缀；另有布尔 **`github_oauth_client_id_env`** 表示 `CM_GITHUB_OAUTH_CLIENT_ID` 是否非空，env 优先） |
| `mcp_bearer_{id}` | 远程 MCP 的 `Authorization: Bearer`（按服务器 id；删除服务器时清除钥匙串，并清理遗留明文文件） |
| `saved_model_<sha256>` | 已保存模型的 API Key（`llm_overrides.json` 仅留 `has_api_key`） |

旧 **`$XDG_DATA_HOME/crabmate/secrets/<账户名>`** 明文文件：首次成功读/写钥匙串后自动迁移并删除；钥匙串已有值时仅在遗留文件仍存在时清理。钥匙串不可用时：有遗留文件则保留并报错；无遗留文件则降噪（debug），避免刷屏。

**禁止**写入 `prefs.json` / `web_sessions.json` / 日志 / `doctor` 明文输出。

---

## 5. 与工作区 `.crabmate/` 的边界

| 数据 | 位置 | 三端 |
|------|------|------|
| 供应商对话消息链、`conversation_id` | `<workspace>/.crabmate/conversations.db` | Web `serve`、TUI（配置非空时）、**不**迁入 home |
| 导出 JSON/Markdown | `<workspace>/.crabmate/exports/` | Web / `save-session` |
| REPL 行历史 | `<workspace>/.crabmate/repl_history.txt` | REPL only |
| TUI 单链快照（无 SQLite 时） | `<workspace>/.crabmate/tui_session.json` | TUI；Phase 2 可选与 `web_sessions` 对齐 |
| 侧栏多会话、壳层偏好、本机 LLM 覆盖 | **`~/.local/share/crabmate`** | Web + Tauri + CLI 读 |

---

## 6. 三端访问架构

```mermaid
flowchart TB
  subgraph disk ["~/.local/share/crabmate"]
    P[prefs.json]
    L[llm_overrides.json]
    W[workspaces/hash/web_sessions.json]
    S[secrets/]
  end

  subgraph rust ["Rust user_data 模块"]
    IO[读写 + 文件锁 + schema 校验]
  end

  CLI[CLI / REPL / TUI]
  WEB[Web WASM]
  TAU[Tauri WebView]

  IO --> disk
  CLI --> IO
  WEB --> API["HTTP /user-data/*"]
  TAU --> API
  API --> IO
```

| 端 | 方式 | 说明 |
|----|------|------|
| **Web** | `GET/PUT /user-data/*` | WASM 无法直接读 `$HOME`；与现有 Bearer 鉴权一致 |
| **Tauri** | 同 Web（`serve` 动态 loopback URL，见 **`web_ready` JSON**） | 业务数据不再依赖 `com.crabmate.desktop/localstorage/` |
| **CLI** | `user_data` 直读；`doctor` 打印路径与钥匙串脱敏状态 | 启动**不**自动套用 `prefs.last_workspace_root`（须 `--workspace`）；`cm_role` 仍可回退；密钥优先 `API_KEY` env，其次系统钥匙串 |
| **TUI** | 直读 `prefs` + 可选 HTTP | 同 CLI：不自动打开上次工作区；会话链仍以 SQLite / `tui_session.json` 为主 |

---

## 7. HTTP API（草案）

挂载于受保护路由（与 `/chat`、`/workspace` 同级），前缀 **`/user-data`**：

| 方法 | 路径 | 作用 |
|------|------|------|
| `GET` | `/user-data/prefs` | 读 `prefs.json` |
| `PUT` | `/user-data/prefs` | 写回；可选 `If-Match` / revision |
| `GET` | `/user-data/llm-overrides` | 读 `llm_overrides.json` |
| `PUT` | `/user-data/llm-overrides` | 写回非机密 LLM 字段 |
| `PUT` | `/user-data/secrets/client-llm` | 仅写系统钥匙串；**无**对应 GET 明文 |
| `GET` | `/user-data/secrets/status` | `{ "client_llm": { "set": true }, ... }` 脱敏状态 |
| `GET` | `/user-data/workspaces/current/sessions` | 按当前 `workspace_override` 解析桶 |
| `PUT` | `/user-data/workspaces/current/sessions` | 写 `web_sessions.json` |
| `GET` | `/user-data/workspaces` | 列出 `manifest.json` |
**工作区未设置**：`current` 映射到 `global/web_sessions.json`。

---

## 8. LLM 配置合并优先级（运行时）

对单次 `POST /chat` / `POST /chat/stream`：

**非密钥字段**（`api_base` / `model` / 上下文窗口等）：请求体优先；空缺由 **`llm_overrides.json`** 填补；再回落到 `AgentConfig` / TOML。

**`api_key`（高 → 低，与 CLI 一致）**：

1. 请求体 **`client_llm.api_key`**（Web 设置页当次提交，**不写盘**除非用户显式保存）  
2. 进程环境 **`API_KEY`**（非空时**不**再注入钥匙串，便于临时覆盖）  
3. 已保存模型钥匙串 → **`client_llm`** 钥匙串（服务名 `com.crabmate.credentials`）

与现文档一致：持久化密钥只进系统钥匙串，**服务端 `serve` 进程不把 Web 密钥写入 `AppState` 持久字段**（启动时仍可读 env/`API_KEY`）。旧 `secrets/*` 与 `saved_models[*].api_key` 采用「先写钥匙串、成功后删明文」迁移；失败时保留旧数据。

---

## 9. 迁移（已废弃）

不再提供 `POST /user-data/migrate` 与 `localStorage` 回退；请直接使用 **`~/.local/share/crabmate`** 或设置 **`CM_CRABMATE_USER_DATA_DIR`**。

---

## 10. 安全与运维

- 创建目录 **`0700`**，`secrets/` 下文件 **`0600`**。
- API 与 `/chat` 相同 Bearer；日志禁止打印 `secrets/` 与 `sessions` 全文。
- `GET` 类接口**不得**返回完整 `api_key`（允许 `has_key`、`key_suffix` 等脱敏字段）。
- 备份：复制 `~/.local/share/crabmate` 可备份侧栏会话与偏好；**系统钥匙串中的密钥需另行导出/备份**（目录内遗留 `secrets/` 明文若仍在则也含密钥）。勿将目录提交到 git 或公开网盘。
- 多 `serve` 实例：对 `web_sessions.json` 使用文件锁或单写者，避免并发写坏。

环境变量：

| 变量 | 说明 |
|------|------|
| `CM_CRABMATE_USER_DATA_DIR` | 覆盖本机用户数据根（绝对路径） |

---

## 11. 实现分期（建议）

| 阶段 | 内容 | 验收 |
|------|------|------|
| **P0** | Rust `user_data` 模块；`prefs` / `web_sessions` / `llm_overrides` 读写；`doctor` 显示路径 | 可手工编辑 JSON，CLI 可读 |
| **P1** | HTTP `/user-data/*`；Web 会话列表改 HTTP；`last_workspace_root` | Web + Tauri 共用；告别 Tauri localStorage 分叉 |
| **P2** | 壳层 prefs 迁出 `localStorage`；`migrate` 端点 | 主题/侧栏跨实例一致 |
| **P3** | `secrets/` + 脱敏 API；CLI 可选读 secrets；TUI 读 `prefs` | 三端 LLM URL/模型/密钥策略一致 |
| **P4** | OpenAPI、`docs/配置说明.md`、e2e 使用临时 `CM_CRABMATE_USER_DATA_DIR` | 可测、可文档化 |

实现位置：

- `src/user_data/`、`src/web/user_data/`、`frontend/src/api/user_data.rs`、`frontend/src/user_prefs_sync.rs`

---

## 12. 与 Tauri 的关系（补充）

当前 Client 仓桌面壳（**`../crabmate-client/desktop-tauri`**）**不**再 spawn `serve`。请自行启动 **`crabmate serve`**（开发时 cwd 多为仓库根；安装后可用 **`CM_WEB_STATIC_DIR=/usr/share/crabmate/frontend/dist`** 指向 deb 内静态资源）。壳通过连接页或 **`CM_DESKTOP_SERVE_URL`** 加载 WebView（**勿**假设固定端口如 3000，除非你自己用该端口启动）。  
WebView 连上后，**用户级**状态应由 **`/user-data`** 读写，而非 `~/.local/share/com.crabmate.desktop/localstorage/`。

详见 Client 仓 **`docs/design/tauri_gui_mvp_design.md`**（进程壳层；主仓本路径为指针）与 **`../crabmate-client/desktop-tauri/DEVELOPMENT.md`**（开发与故障排查）；本文件负责**数据落盘**。

---

## 13. 参考

- `frontend/src/storage.rs` — 会话分桶与 `ChatSession` 形状  
- `frontend/src/api/client_llm_storage.rs` — LLM 覆盖经 `/user-data/llm-overrides` 与 `secrets/`  
- `docs/命令行与路由.md` — CLI 与 Web 会话持久对照  
- `docs/配置说明.md` — `API_KEY`、`client_llm`、鉴权  
- `.cursor/rules/secrets-and-logging.mdc` — 密钥与日志  
- `.cursor/rules/api-sse-chat-protocol.mdc` — HTTP 变更须同步前端  
