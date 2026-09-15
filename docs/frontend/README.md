# Web UI 文档（本仓指针）

业务 UI **源码**已迁至官方 Client 仓：

- 仓：[`noisystreet/crabmate-client`](https://github.com/noisystreet/crabmate-client)
- 构建：先将 [crabmate-client](https://github.com/noisystreet/crabmate-client) 克隆为同级后 `cd ../crabmate-client && make frontend`
- `serve`：永远纯 API，不托管 SPA；浏览器 UI 由 Client 仓自行托管，经 **CORS**（`CM_WEB_CORS_ALLOWED_ORIGINS`）接入

迁出计划见 [`../design/frontend_migrate_plan.md`](../design/frontend_migrate_plan.md)。
