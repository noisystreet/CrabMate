# 基于现有 Web UI 的 Tauri GUI 设计（MVP）

## 1. 目标

在不重构现有业务逻辑的前提下，为项目新增一个桌面 GUI：

- 复用已有 `frontend` Web UI
- 复用已有 Rust 后端 `serve` 模式
- 通过 Tauri 提供桌面应用壳层与进程管理

MVP 验收标准：

1. 启动桌面应用后自动拉起后端服务
2. WebView 打开本地服务地址并可正常使用
3. 关闭桌面应用时后端进程可回收
4. 保持后端仅监听 loopback（`127.0.0.1`）

## 2. 架构方案

采用“Web 壳 + 本地后端进程”模式：

1. Tauri 启动后端进程（`crabmate serve`）
2. 后端在 ready 后输出一行机器可读 JSON（包含端口）
3. Tauri 解析该 JSON 并加载 `http://127.0.0.1:<port>`
4. 前端继续沿用现有 SSE/HTTP API

该方案的核心优点：

- 复用最大化，落地快
- 风险集中在启动握手与进程生命周期
- 后续可逐步叠加桌面能力（托盘、通知、文件选择、自动更新）

## 3. 代码落地范围

### 3.1 后端（已实现）

CLI `serve` 子命令桌面握手：

- 参数：**`--desktop-ready-json`**
- 行为：当 **`TcpListener::bind`** 成功后，向 stdout 额外打印一行 JSON（基于 **`local_addr()`**）：

```json
{"event":"web_ready","host":"127.0.0.1","port":37007,"url":"http://127.0.0.1:37007/","auth_enabled":false}
```

说明：

- 该输出**仅**在显式开启 **`--desktop-ready-json`** 时出现
- 支持 **`--port 0`** 随机端口；**`port`/`url`** 字段为实际绑定地址
- 实现：`src/cli_run.rs`（`run_serve_branch`）、`crates/crabmate-config`（`ServeCmd`）

### 3.2 桌面端（已实现）

`desktop-tauri/` 工程：

- `desktop-tauri/src-tauri/src/main.rs` — 启动 **`serve --host 127.0.0.1 --port 0 --desktop-ready-json`**，解析 **`web_ready`**，加载 WebView，退出时 kill 子进程
- `desktop-tauri/scripts/prepare-sidecar.sh` — 打包前复制 **`crabmate`** sidecar
- **`desktop-tauri/README.md`**、**`desktop-tauri/DEVELOPMENT.md`** — 开发与故障排查

**勿**再使用「固定 **3000** + TCP 探测」作为就绪条件（会误连本机其它旧 **`serve`** 进程，导致 API 405/404）。

## 4. 实施步骤（MVP）

1. ~~后端新增 `--desktop-ready-json` 参数与 ready 输出~~（已完成）
2. ~~Tauri 启动 `crabmate serve --host 127.0.0.1 --port 0 --desktop-ready-json`~~（已完成）
3. ~~解析 ready JSON、加载动态 URL、退出时回收子进程~~（已完成）
4. 文档与 **`frontend/dist`** / sidecar 发版流程与代码同 PR 维护（见 **`desktop-tauri/DEVELOPMENT.md`** § 发布检查清单）

### 开发启动命令（当前实现）

1. 在仓库根目录编译后端并构建前端（Tauri WebView 由 **`serve`** 提供 **`frontend/dist`**）：

```bash
cd /path/to/crabmate_agent
cargo build
cd frontend && trunk build && cd ..
```

2. 启动 Tauri 开发界面（显式指定后端可执行文件路径）：

```bash
cd /path/to/crabmate_agent/desktop-tauri/src-tauri
CM_DESKTOP_BACKEND_BIN=/path/to/crabmate_agent/target/debug/crabmate cargo tauri dev
```

3. 若未安装 Tauri CLI，先安装：

```bash
cargo install tauri-cli --version "^2"
```

启动日志中应出现 **`{"event":"web_ready",…}`**；WebView URL 须与该 JSON 的 **`url`** 一致。

## 5. 安全基线

- 桌面模式默认仅 loopback 监听
- 不自动放开 `0.0.0.0`
- 若启用鉴权，token 不写入日志明文（后续可接 keyring）

## 6. 风险与缓解

1. 进程管理复杂度提升：
   - 缓解：统一由 Tauri 生命周期管理并在退出时强制回收
2. 后端输出协议不稳定：
   - 缓解：ready JSON 固定字段，后续加版本号
3. 端口冲突/竞争：
   - 缓解：支持 `--port 0`，由系统分配并回传真实端口
