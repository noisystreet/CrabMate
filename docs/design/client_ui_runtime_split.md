# 设计：Client 承载 UI · Server 默认纯 API（运行时拆分）

> **状态**：进行中（Phase 1 已在本仓落地默认纯 API）  
> **对齐**：ADR [路径 A](./client_shell_split.md) — 本仓终点态以 **`serve` + 契约** 为主；业务 UI 由 Client 构建与发版。  
> **身份**：进程内不做多用户登录；门禁用 Web Bearer 或网关/BFF（见 **`docs/未来规划功能.md`**）。  
> **草稿出处**：本地 `agent_space/client-ui-server-api-split-plan.md`（成熟后以本文为准）。

---

## 1. 目标

**Server（`crabmate serve`）只提供 HTTP/SSE API 与 Agent 执行权威；业务 UI 的构建、发版与（目标态）运行时加载均在 Client 侧。**

- **`serve` 默认即为纯 API**。
- 同机托管静态 UI 须 **显式** **`--with-web`** / **`--web`**（配合 **`CM_WEB_STATIC_DIR`** 或可探测 dist）。
- **`--no-web` / `--cli-only` 已删除**（曾为与默认等价的无操作旗标）。

---

## 2. 分阶段

| 阶段 | 内容 | 本仓状态 |
|------|------|----------|
| **Phase 1** | UI/API 分 Origin；`serve` 默认不挂 SPA；CORS 暴露头；文档/ systemd / man | **已落地**（CLI + 文档） |
| **Phase 2** | Desktop / Android 包内或本地加载业务 UI，API 基址指向 `serve` | **进行中**（Client：`connect_remote` → 包内 `index.html` + `cm_api_base`） |
| **Phase 3** | 叙事收敛；可选浏览器 session-only Bearer、网关示例 | 按需 |

公网分入口 / VPN 等运维收口**不是**本设计前置（可并行，见个人云附录）。

---

## 3. Phase 1 退出标准（Server）

- [x] 默认 `serve` 不托管 SPA（`mount_web_ui=false`）
- [x] **`--with-web`** 显式兼容托管
- [x] 删除无操作旗标 **`--no-web` / `--cli-only`**
- [x] README / 配置说明 / 命令行与路由 / systemd 注释 / CI 断言与行为一致
- [x] 冒烟 / 真 LLM / AGENTS 等同机托管路径改为 **`--with-web`**（跨 Origin §9 仍纯 API）
- [x] CORS 已暴露 **`x-conversation-id` / `x-stream-job-id` / `x-request-id`**（既有实现；官方壳 Origin 默认放行，额外静态 Origin 用 **`CM_WEB_CORS_ALLOWED_ORIGINS`**）
- [x] 纯 API 热路径：未传 `--with-web` 时不探测/解析 `frontend` dist
- [ ] Client 仓 E2E/README 启动命令同步加 **`--with-web`**（本仓外）
- [ ] E2E / 冒烟在「静态私有或壳本地」路径上通过（随 Client / 部署验证）

---

## 4. 推荐个人云拓扑（Phase 1）

```
公网 DNS:  api.…  → VPS Caddy → 127.0.0.1:8080  # 默认纯 API 的 serve
静态 UI:   不配公网 A；壳包内 / Tailscale 私有 Origin / 过渡期 --with-web
```

跨 Origin：官方壳（`tauri://localhost` / `http://tauri.localhost`）**默认已放行**；其它浏览器静态 Origin 写入 **`CM_WEB_CORS_ALLOWED_ORIGINS`**（与默认合并；精确白名单，禁止 `*`；改后重启 `serve`）。无头 VPS 密钥用 **`EnvironmentFile` / systemd credentials**，勿依赖 gnome-keyring。

---

## 5. 参考

- [`client_shell_split.md`](./client_shell_split.md) — 路径 A ADR  
- [`client_shell_split_todo.md`](./client_shell_split_todo.md) — 执行清单  
- [`client_contract_versioning.md`](./client_contract_versioning.md) — 契约发版  
- [`../配置说明.md`](../配置说明.md) — `CM_WEB_STATIC_DIR`、CORS、Bearer  
- [`../命令行与路由.md`](../命令行与路由.md) — `serve --with-web`  
