# Client ↔ Server 兼容矩阵（路径 A）

> **状态**：Phase 4.4 初稿（2026-08-08）  
> **关联**：[`client_contract_versioning.md`](./client_contract_versioning.md)、[`client_shell_split.md`](./client_shell_split.md)、Client 仓 [contract_pin.md](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/contract_pin.md)（本机同级亦可 `../crabmate-client/docs/design/contract_pin.md`）

本表描述 **本仓 `serve` / 契约 crate** 与 **官方 Client（`crabmate-client`）** 的最低对齐要求。破坏性变更须 bump 线协议或契约 semver，并更新本表。

## 1. 当前基线

| 轴 | 当前值 | 权威位置 |
|----|--------|----------|
| 线协议 `SSE_PROTOCOL_VERSION` | **2** | `crabmate-sse-protocol` / `docs/SSE协议.md` |
| 契约 git tag | `client-contract-v0.2.0`（当前；前序 `v0.1.1` / `v0.1.0`） | [`client_contract_versioning.md`](./client_contract_versioning.md) |
| 官方 Client 仓 | `noisystreet/crabmate-client` | 同级 `../crabmate-client` |
| N−1 线协议解码窗口 | **无**（错位即失败可预期） | versioning §3 |

## 2. 矩阵（发版时填写）

| Server / 契约 | 最低 Client（壳） | 最低 UI（WASM/dist） | 备注 |
|---------------|-------------------|----------------------|------|
| 本仓 `main` + SSE v2 + `client-contract-v0.2.0` | Client 仓钉同 tag（线契约）；`crabmate-tool-card` 须为本仓 path，勿再 git 钉本 tag | Client `frontend/dist`（`make frontend`） | W2b：快照 `role=tool` 无 `display_*`；Client 本地水合。跨 Origin 须 CORS + Web Bearer |
| 本仓历史 + `client-contract-v0.1.1` / 产品 `v0.3.0` | Client 可 git 钉 Server `crabmate-tool-card` | 同左 | 仍含本仓 `tool-card`；快照可带 `display_*` |
| 本仓历史 + `client-contract-v0.1.0` | 同左 | 同左 | 契约 crate 形状与 `v0.1.1` 同；不含 D2.1 `serve`/CLI 行为 |

**最低 Client** 含义：连接页 + WebView 能完成一轮对话，且协议错位返回 `SSE_CLIENT_TOO_NEW` / `SSE_PROTOCOL_MISMATCH` 可预期。

## 3. 安装包边界

| 产物 | 来源仓 | 含什么 |
|------|--------|--------|
| `crabmate` / `serve`（tar.gz / server `.deb`） | **本仓** | 二进制 + 配置模板；可选附带 UI dist；同进程 `chat|repl|tui` **命令入口已移除**（D2.1） |
| Desktop Linux `.deb` / Android APK | **`crabmate-client`** | 壳 + connect；**不**内嵌 `serve` sidecar |
| 业务 UI 静态包 | **`crabmate-client/frontend`** | `index.html` + wasm 等 |
| **`crabmate-tui`**（远程终端） | **`crabmate-client`** | **官方** HTTP/SSE 终端；**不**内嵌 / spawn `serve`；见 Client [`remote_cli_tui.md`](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/remote_cli_tui.md) |

## 4. 弃用

| 项 | 状态 |
|----|------|
| 主仓 `desktop-tauri` / `mobile-tauri` / `crabmate-connect` | **已移除**（Phase 4.1；权威仅在 `crabmate-client`） |
| `serve --desktop-ready-json` | **保留别名** `--web-ready-json`；壳不依赖；脚本仍可用 |
| 本仓同进程 `crabmate chat|repl|tui` | **命令入口已移除**（D2.1）；请用 Client **`crabmate-tui`** + `serve`；实现硬删见 [`client_shell_split.md`](./client_shell_split.md) §2.5 **D2.2** |
