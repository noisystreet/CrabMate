# Web 宿主提取（modular monolith）

## 目标

在**不**拆独立部署进程的前提下，把 Web 控制面与领域回合解耦：handler 只拿所需 **facet**；**`crabmate-web-host`** 承载 HTTP 契约与 serve 壳，**不得**依赖整包 `crabmate-internal`。

> 命名：Axum 宿主包为 **`crabmate-web-host`**。仓库成员 **`crabmate-web`**（`frontend/`）是 Leptos WASM UI，二者勿混用。

## 非目标

- 独立 Web 微服务 / 独立仓库
- 改 SSE 行协议或对外 HTTP 契约字段
- 在**无**清晰依赖注入时把带状态 handler / 整包 `AppState` 硬迁入 web-host（孤儿规则仍适用）

## 可评估（P2 后）

- **`chat_job_queue`（或 worker）迁出根包 / 贴近 web-host**：`WebChatQueueDeps` 已注入 [`TurnRunner`](./turn_host_decouple.md)，queue **不再**硬连 `run_agent_turn`。迁模块时仍须处理：`AppState` / Facet 与 handler 同 crate（孤儿规则）、以及避免 web-host → internal。**尚未迁**；仅解除「必然循环」的调用边障碍。

## 阶段

| 阶段 | 内容 | 状态 |
|------|------|------|
| **A** | handler `FromRef` + 更细 facet；`AppStateHttpCore` 工作区路径解析 | **完成**（控制面 `WebChatAppFacet`；回合 `WebChatTurnAppFacet`；见 turn_host P3b/P3c） |
| **B** | **`crabmate-web-host`**：HTTP DTO、`chat_keys`、`limits`、`GET /web-ui` | **完成** |
| **C** | 根包 `build_app` 只装配路由/`AppState`；体积分层与静态挂载走 `web-host::serve` | **完成** |

### 为何 handler 未整包迁入 web-host

axum `FromRef<Arc<AppState>> for Facet` 要求 **Facet 与 AppState 同 crate**（孤儿规则）。`AppState` 仍在根包，故带状态的 handler 留在 `src/web/`；web-host 专责契约与 serve 壳。

回合执行面解耦（`ToolDispatch` / `TurnRunner`）见 **`docs/design/turn_host_decouple.md`**（P1/P2 已在根包落地注入边界）。

## 依赖方向

```text
serve / cli_run（根包）
  → 装配 AppState、域路由、鉴权中间件、DefaultTurnRunner
  → crabmate_web_host::serve::{layer_protected_body_limit, mount_uploads_and_spa}
  → crabmate_web_host::routes::web_ui / http_types / …
```

禁止边（`scripts/check-crate-deps.sh`）：**`crabmate-web-host` ↛ `crabmate-internal`**。

## 验证

- `cargo check` / `cargo clippy`（根包）
- `bash scripts/check-crate-deps.sh`（亦经 pre-commit / CI）
