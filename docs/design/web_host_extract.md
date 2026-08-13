# Web 宿主提取（modular monolith）

## 目标

在**不**拆独立部署进程的前提下，把 Web 控制面与领域回合解耦：handler 只拿所需 **facet**；**`crabmate-web-host`** 承载 HTTP 契约与 serve 壳，**不得**依赖整包 `crabmate-internal`。

> 命名：Axum 宿主包为 **`crabmate-web-host`**。Leptos WASM UI 是 Client 仓 **`crabmate-web`**（`../crabmate-client/frontend`），二者勿混用。

## 非目标

- 独立 Web 微服务 / 独立仓库
- 改 SSE 行协议或对外 HTTP 契约字段
- 在**无**清晰依赖注入时把带状态 handler / 整包 `AppState` 硬迁入 web-host（孤儿规则仍适用）

## P5 评估结论（2026-08-08）

- **`chat_job_queue` / 带状态 chat handler**：**暂不**整包迁入 `crabmate-web-host`。  
- 调用边已解（`TurnRunner` 注入）；模块边仍受 **根包 ↔ web-host 循环风险**、**`web-host ↛ internal`**、**`FromRef` 孤儿规则** 阻挡。  
- 「贴近」= 契约/无状态壳继续进 web-host + 根包内 facet 收窄；详见 **[`web_host_p5_placement.md`](./web_host_p5_placement.md)**。

## 阶段

| 阶段 | 内容 | 状态 |
|------|------|------|
| **A** | handler `FromRef` + 更细 facet；`AppStateHttpCore` 工作区路径解析 | **完成**（控制面 `WebChatAppFacet`；回合 `WebChatTurnAppFacet`；E2E `E2eConversationFixtureFacet`；见 turn_host P3b/P3c） |
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
