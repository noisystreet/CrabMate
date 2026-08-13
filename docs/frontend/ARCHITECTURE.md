# Web UI 架构（指针）

官方 Leptos / WASM UI 源码与架构说明在 Client 仓：

- 仓：[`noisystreet/crabmate-client`](https://github.com/noisystreet/crabmate-client)
- 前端：`../crabmate-client/frontend`
- 构建：`cd ../crabmate-client && make frontend`

本 Server 仓只维护 HTTP/SSE 契约（[`docs/SSE协议.md`](../SSE协议.md)、[`docs/Turn布局设计.md`](../Turn布局设计.md)）与可选 **`serve --with-web`** 静态托管。不要在本仓查找 `frontend/src/`。
