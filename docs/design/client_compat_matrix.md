# Client ↔ Server 兼容矩阵（路径 A）

> **状态**：Phase 4.4 初稿（2026-08-08）  
> **关联**：[`client_contract_versioning.md`](./client_contract_versioning.md)、[`client_shell_split.md`](./client_shell_split.md)、Client 仓 [contract_pin.md](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/contract_pin.md)（本机同级亦可 `../crabmate-client/docs/design/contract_pin.md`）

本表描述 **本仓 `serve` / 契约 crate** 与 **官方 Client（`crabmate-client`）** 的最低对齐要求。破坏性变更须 bump 线协议或契约 semver，并更新本表。

## 1. 当前基线

| 轴 | 当前值 | 权威位置 |
|----|--------|----------|
| 线协议 `SSE_PROTOCOL_VERSION` | **2** | `crabmate-sse-protocol` / `docs/SSE协议.md` |
| 契约 git tag（计划） | `client-contract-vX.Y.Z` | [`client_contract_versioning.md`](./client_contract_versioning.md) |
| 官方 Client 仓 | `noisystreet/crabmate-client` | 同级 `../crabmate-client` |
| N−1 线协议解码窗口 | **无**（错位即失败可预期） | versioning §3 |

## 2. 矩阵（发版时填写）

| Server / 契约 | 最低 Client（壳） | 最低 UI（WASM/dist） | 备注 |
|---------------|-------------------|----------------------|------|
| 本仓 `main` + SSE v2 | Client 仓与本表同日基线；须能发 `client_sse_protocol: 2` | 与 Server 同协议版本构建的 `frontend/dist`（过渡期仍可由本仓 `trunk build`） | 跨 Origin 须 CORS + Web Bearer |
| 未来 `client-contract-v*` | 钉该 tag 的壳 / connect | 钉同协议版本的 UI 产物 | 首枚 tag 打出后补一行 |

**最低 Client** 含义：连接页 + WebView 能完成一轮对话，且协议错位返回 `SSE_CLIENT_TOO_NEW` / `SSE_PROTOCOL_MISMATCH` 可预期。

## 3. 安装包边界

| 产物 | 来源仓 | 含什么 |
|------|--------|--------|
| `crabmate` CLI / `serve`（tar.gz / server `.deb`） | **本仓** | 二进制 + 配置模板；可选附带 `frontend/dist`（至 Phase 4.2/4.3） |
| Desktop Linux `.deb` / Android APK | **`crabmate-client`** | 壳 + connect；**不**内嵌 `serve` sidecar |
| 业务 UI 静态包 | 过渡：本仓 `frontend/`；终点：Client 或独立 UI 仓（Phase 5） | `index.html` + wasm 等 |

## 4. 弃用

| 项 | 状态 |
|----|------|
| 主仓 `desktop-tauri` / `mobile-tauri` / `crabmate-connect` | **已移除**（Phase 4.1；权威仅在 `crabmate-client`） |
| `serve --desktop-ready-json` | **保留别名** `--web-ready-json`；壳不依赖；脚本仍可用 |
