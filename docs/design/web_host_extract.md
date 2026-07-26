# Web 宿主提取（modular monolith）

## 目标

在**不**拆独立部署进程的前提下，把 Web 控制面与领域回合解耦：handler 只拿所需 **facet**；最终 **`crabmate-web-host`** 只依赖 turn 入口、SSE、config/types，**不得**依赖整包 `crabmate-internal`。

> 命名：Axum 宿主包为 **`crabmate-web-host`**。仓库成员 **`crabmate-web`**（`frontend/`）是 Leptos WASM UI，二者勿混用。

## 非目标（本阶段）

- 独立 Web 微服务 / 独立仓库
- 改 SSE 行协议或对外 HTTP 契约字段
- 大爆炸整包搬迁 `src/web/`

## 阶段

| 阶段 | 内容 | 状态 |
|------|------|------|
| **A** | handler `FromRef` + 更细 facet；`AppStateHttpCore` 承载工作区路径解析 | **进行中**（config_reload / upload / tasks / workspace / skills / health / status / changelog / github / user_data / auth） |
| **B** | 新建 **`crabmate-web-host`**：先迁 HTTP DTO，再迁 routes/handlers / `chat_job_queue` | **进行中**（`skills` / `workspace` / `github` 信封 / `api`：ApiError·upload·config reload） |
| **C** | 根包 `serve` 只装配 `AppState` + router + 静态 UI | 未做 |

## A 阶段约定

- Router 状态仍为 **`Arc<AppState>`**（与现网一致）。
- 窄 handler 用 **`State<SomeFacet>`** / **`State<AppStateHttpCore>`**，经 **`FromRef<Arc<AppState>>`** 投影；宽入口（chat/stream）可暂留整包。
- Facet 命名：`*Facet` / `*AppFacet`；与队列侧 **`WebChatJobAppFacet`** 同族。

## 依赖方向（目标 DAG 摘要）

```text
serve (root)
  → crabmate-web-host (handlers / routes / queue glue / HTTP DTOs)
       → crabmate-agent | crabmate-sse-protocol | crabmate-config | crabmate-types | …
  → AppState 装配（workspace / ProcessHandles / …）
```

禁止边（`scripts/check-crate-deps.sh`）：**`crabmate-web-host` ↛ `crabmate-internal`**。

## 验证

- `cargo check` / `cargo clippy`（根包）
- `bash scripts/check-crate-deps.sh`
- 行为不变：窄路由 HTTP 契约与鉴权不变
