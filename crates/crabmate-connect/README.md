# crabmate-connect

桌面 / 移动 **Tauri 2** 壳共用的「连接远程 `crabmate serve`」逻辑（探测、Bearer hash 交接、钥匙串）。

**不**在根 Cargo workspace 的 `members` 中（根 `exclude` + 本包空 `[workspace]`，避免主 CI 拉 GTK/WebKit）；由 `desktop-tauri` / `mobile-tauri` 或未来独立壳仓引用。

## 版本与钉依赖

权威策略：[docs/design/client_contract_versioning.md](../../docs/design/client_contract_versioning.md)。

- Crate **`version`**：本目录 `Cargo.toml`（semver）。
- **`publish = false`**：默认用 **git 标签** `client-contract-vX.Y.Z`，不要走 crates.io。
- **Tauri**：依赖 **`tauri = "2"`**；与官方壳 major 对齐，升级须同步改壳。

外仓示例（拆壳后）：

```toml
crabmate-connect = { git = "https://github.com/noisystreet/CrabMate", tag = "client-contract-v0.1.0", path = "crates/crabmate-connect" }
```

本仓过渡期：

```toml
crabmate-connect = { path = "../../crates/crabmate-connect" }
```

## 能力边界

- 探测 `GET /health` 与受保护的 prefs；非空 Bearer 经 `#cm_web_api_bearer=` 交给前端。
- **不**实现聊天 / SSE；线协议版本错位由 UI 与 `serve` 按 `SSE_PROTOCOL_VERSION` 与稳定错误码处理（见 `docs/SSE协议.md`）。

静态页：`assets/connect.html`（壳侧脚本同步进 `dist/`）。
