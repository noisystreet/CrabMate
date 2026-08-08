# 基于现有 Web UI 的 Tauri GUI 设计（MVP）

## 1. 目标

在不重构现有业务逻辑的前提下，为项目新增一个桌面 GUI：

- 复用已有 `frontend` Web UI
- 复用已有 Rust 后端 `serve` 模式
- 通过 Tauri 提供桌面应用壳层与进程管理

MVP 验收标准：

1. 启动桌面应用后自动拉起后端服务
2. WebView 先展示连接页（服务器 + Bearer；与移动端共用 `crabmate-connect`），确认后打开 `serve` UI；E2E/`CM_DESKTOP_SKIP_CONNECT` 可跳过
3. 主窗口可最小化到系统托盘；关窗或显式退出时后端进程可回收
4. 保持后端仅监听 loopback（`127.0.0.1`）
5. 桌面应用保持单实例，重复启动只唤醒已有窗口

## 2. 架构方案

采用“Web 壳 + 本地后端进程”模式：

1. Tauri 启动后端进程（`crabmate serve`）
2. 后端在 ready 后输出一行机器可读 JSON（包含端口）
3. Tauri 解析该 JSON，打开连接页并预填该 URL；用户确认（或自动重连）后再加载 `http://127.0.0.1:<port>`（亦可改填远程 `serve`）
4. 前端继续沿用现有 SSE/HTTP API；首次非空 Bearer 写入本机钥匙串 `tauri_connect_web_api_bearer`

该方案的核心优点：

- 复用最大化，落地快
- 风险集中在启动握手与进程生命周期
- 后续可逐步叠加桌面能力（通知、文件选择、自动更新；托盘、单实例与**启动闪屏进度/失败页**已实现）

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

- `desktop-tauri/src-tauri/src/main.rs` — 启动 **`serve --host 127.0.0.1 --port 0 --desktop-ready-json`**，解析 **`web_ready`**，加载 WebView，显式退出时 kill 子进程
- `desktop-tauri/src-tauri/src/desktop_lifecycle.rs` — 单实例唤醒、系统托盘、主窗口最小化隐藏；托盘不可用时保留普通最小化
- `tauri-plugin-window-state` — 按稳定标签 `main` 保存/恢复窗口大小、位置与最大化状态；排除启动闪屏且不恢复可见性，避免托盘退出后下次隐藏启动
- `desktop-tauri/scripts/prepare-sidecar.sh` — 打包前复制 **`crabmate`** sidecar
- **`desktop-tauri/README.md`**、**`desktop-tauri/DEVELOPMENT.md`** — 开发与故障排查

**勿**再使用「固定 **3000** + TCP 探测」作为就绪条件（会误连本机其它旧 **`serve`** 进程，导致 API 405/404）。

## 4. 实施步骤（MVP）

1. ~~后端新增 `--desktop-ready-json` 参数与 ready 输出~~（已完成）
2. ~~Tauri 启动 `crabmate serve --host 127.0.0.1 --port 0 --desktop-ready-json`~~（已完成）
3. ~~解析 ready JSON、加载动态 URL、退出时回收子进程~~（已完成）
4. ~~单实例保护、系统托盘与最小化隐藏~~（已完成）
5. 文档与 **`frontend/dist`** / sidecar 发版流程与代码同 PR 维护（见 **`desktop-tauri/DEVELOPMENT.md`** § 发布检查清单）

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
   - 缓解：统一由 Tauri 生命周期管理；最小化隐藏不回收，关窗、显式退出或系统退出时强制回收
2. 后端输出协议不稳定：
   - 缓解：ready JSON 固定字段，后续加版本号
3. 端口冲突/竞争：
   - 缓解：支持 `--port 0`，由系统分配并回传真实端口；单实例插件避免同一桌面应用重复拉起后端
4. 无系统托盘的桌面环境中窗口隐藏后不可恢复：
   - 缓解：仅在托盘初始化成功时将最小化改为隐藏；初始化失败时保留普通最小化
