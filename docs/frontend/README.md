# Web UI 文档（本仓指针）

业务 UI **源码**已迁至官方 Client 仓：

- 仓：[`noisystreet/crabmate-client`](https://github.com/noisystreet/crabmate-client)
- 前端：[`frontend/`](https://github.com/noisystreet/crabmate-client/tree/main/frontend)（Leptos / WASM；`app/chat/`、`composer_stream`、`wire_*` 等）
- 构建：先将 [crabmate-client](https://github.com/noisystreet/crabmate-client) 克隆为同级后 `cd ../crabmate-client && make frontend`
- `serve`：永远纯 API，不托管 SPA；浏览器 UI 由 Client 仓自行托管，经 **CORS**（`CM_WEB_CORS_ALLOWED_ORIGINS`）接入

本 Server 仓只维护 HTTP/SSE 契约（[`docs/SSE协议.md`](../SSE协议.md)、[`docs/Turn布局设计.md`](../Turn布局设计.md)）；不要在本仓查找 `frontend/src/`。视觉 / 布局手测清单随 UI 源码在 Client 仓维护，本仓不维护逐项勾选。
