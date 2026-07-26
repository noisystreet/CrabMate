# Web 宿主提取（modular monolith）

## 目标

在**不**拆独立部署进程的前提下，把 Web 控制面与领域回合解耦：handler 只拿所需 **facet**；**`crabmate-web-host`** 承载 HTTP 契约与 serve 壳，**不得**依赖整包 `crabmate-internal`。

> 命名：Axum 宿主包为 **`crabmate-web-host`**。仓库成员 **`crabmate-web`**（`frontend/`）是 Leptos WASM UI，二者勿混用。

## 非目标

- 独立 Web 微服务 / 独立仓库
- 改 SSE 行协议或对外 HTTP 契约字段
- 把 `run_agent_turn` / `chat_job_queue` 迁入 web-host（会与根包形成循环依赖）

## 阶段

| 阶段 | 内容 | 状态 |
|------|------|------|
| **A** | handler `FromRef` + 更细 facet；`AppStateHttpCore` 工作区路径解析 | **完成**（chat/stream/async 宽入口仍持整包 `AppState`） |
| **B** | **`crabmate-web-host`**：HTTP DTO、`chat_keys`、`limits`、`GET /web-ui` | **完成** |
| **C** | 根包 `build_app` 只装配路由/`AppState`；体积分层与静态挂载走 `web-host::serve` | **完成** |

### 为何 handler 未整包迁入 web-host

axum `FromRef<Arc<AppState>> for Facet` 要求 **Facet 与 AppState 同 crate**（孤儿规则）。`AppState` / 队列 / `run_agent_turn` 仍在根包，故带状态的 handler 留在 `src/web/`；web-host 专责契约与 serve 壳。

## 依赖方向

```text
serve / cli_run（根包）
  → 装配 AppState、域路由、鉴权中间件
  → crabmate_web_host::serve::{layer_protected_body_limit, mount_uploads_and_spa}
  → crabmate_web_host::routes::web_ui / http_types / …
```

禁止边（`scripts/check-crate-deps.sh`）：**`crabmate-web-host` ↛ `crabmate-internal`**。

## 验证

- `cargo check` / `cargo clippy`（根包）
- `bash scripts/check-crate-deps.sh`
