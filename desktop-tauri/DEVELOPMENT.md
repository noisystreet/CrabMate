# desktop-tauri 开发说明

本文面向本仓库的桌面端开发者，说明如何在本地运行 `desktop-tauri`，以及常见故障和代理/网络配置建议。

## 1. 本地运行

### 1.1 前置依赖

- Rust 工具链（建议 stable）
- Tauri 2 所需系统依赖（按官方文档安装）
- 本仓库后端可执行文件（`crabmate`），用于**自行**启动 `serve`（壳不 spawn）

### 1.2 启动方式（当前实现）

桌面端位于：

- `desktop-tauri/src-tauri`

当前桌面壳**不**拉起 `crabmate serve`。请先在本机或远程启动后端，例如：

```bash
# 仓库根目录（另开终端）
cargo run -- serve --host 127.0.0.1 --port 8080
```

然后启动壳：打开共用**连接页**，预填 **`CM_DESKTOP_SUGGESTED_URL`**（默认 **`http://127.0.0.1:8080/`**）；用户确认后导航到 `serve` UI。

**跳过连接页**须同时满足：

- **`CM_E2E_FIXTURES=1`** 或 **`CM_DESKTOP_SKIP_CONNECT=1`**
- 且设置 **`CM_DESKTOP_SERVE_URL`**（指向已运行的 `serve`）

主 UI 加载在 **`http(s)://…`**（用户填写的本机或远程 `serve`），不是 `tauri://` 资产页。桌面 **`capabilities/default.json`** 的 **`remote.urls` 仅回环**（`127.0.0.1` / `localhost`），以便本机 UI 可 `invoke`；**远程非回环 serve 无桌面 IPC**（外链等走前端 `window.open` 回退）。连接页导航仅放行 `connect_remote` 探测成功后的 **`AllowedServeOrigin`**，避免任意站点留在 WebView 并拿到钥匙串等命令。

### 1.3 常用环境变量

| 变量 | 说明 |
| --- | --- |
| `CM_DESKTOP_SUGGESTED_URL` | 连接页预填（默认 `http://127.0.0.1:8080/`） |
| `CM_DESKTOP_SERVE_URL` | 跳过连接页时必填的已运行 `serve` URL |
| `CM_DESKTOP_SKIP_CONNECT` | 非空且非 `0` 时跳过连接页（须配合上一行） |
| `CM_E2E_FIXTURES` | Victauri E2E：隐藏窗口并跳过连接页（须 `CM_DESKTOP_SERVE_URL`） |

### 1.4 常用开发命令

在 **`desktop-tauri/src-tauri`** 目录：

```bash
cargo check
cargo tauri dev
```

完整链路（仓库根目录）：

```bash
cargo build
cd frontend && trunk build && cd ..
# 终端 A
cargo run -- serve
# 终端 B
cd desktop-tauri/src-tauri && cargo tauri dev
```

发布构建：**`cargo tauri build`**（**`beforeBuildCommand`** 会执行 **`prepare-sidecar.sh`**，仅同步壳静态资源 / 可选 `frontend/dist`，**不**再打包 sidecar 二进制）。

### 1.5 托盘、窗口与单实例

- `tauri-plugin-single-instance` 必须在 Builder 插件序列最前注册。第二实例不会执行 `setup`，而是显示并聚焦已有 `main`；主窗口尚未创建时聚焦 `splash`。
- `src/desktop_lifecycle.rs` 负责托盘和窗口生命周期。托盘菜单含「显示/隐藏」「退出」；Linux 下须使用菜单，Windows/macOS 左键可切换。
- 关闭 `main` 会正常退出壳进程；**不** kill 用户自行启动的 `serve`。托盘初始化成功时，前端最小化命令改为隐藏窗口。
- `tauri-plugin-window-state` 仅跟踪稳定标签 `main`，保存/恢复大小、位置和最大化状态；`splash` 在 denylist 中。刻意不恢复 `VISIBLE`。
- 主窗口先以 `visible(false)` 创建，再在 `show()` 前显式处理几何：闪屏与连接页共用 **480×420**；连上 `serve` 后恢复位置并 **maximize** 会话窗。E2E 直连用完整 `restore_state`。
- 托盘「退出」与 `quit_desktop_app` 共用 `request_desktop_quit`（直接 `app.exit(0)`）。

手动验收：

1. 先启动 `serve`，再开桌面应用；连接页预填本机 URL，连接后进入 UI。
2. 托盘菜单可显示/隐藏主窗口；最小化后本机 `serve` 仍在（由用户管理）。
3. 再次启动同一桌面二进制，确认没有第二个窗口，原窗口被唤醒。
4. 关闭主窗口，确认桌面进程退出且**不影响**已独立运行的 `serve`。
5. 未启动 `serve` 时打开壳，连接页探测失败应有明确提示。
6. 调整主窗口大小、位置、最大化状态后退出；重新启动确认状态恢复。

## 2. 常见故障

### 2.1 连接页无法连通 / 探测失败

原因：

- 本机或远程尚未启动 `crabmate serve`
- URL / 端口错误，或 Web API Bearer 与服务端不一致
- 代理干扰本机回环（调试时 `unset http_proxy https_proxy` 或设 `no_proxy=127.0.0.1,localhost`）

排查：

```bash
curl -sS --noproxy '*' http://127.0.0.1:8080/health
```

### 2.2 启动闪屏报错 / 跳过连接页失败

启动时会先显示 **`splash.html`**。若设置了跳过连接页但未设 **`CM_DESKTOP_SERVE_URL`**，闪屏会报错并提示须先启动 `serve`。

闪屏与连接页静态资源由 **`prepare-sidecar.sh`** 复制到 **`desktop-tauri/dist/splash.html`**、**`connect.html`**（连接页源在 **`crates/crabmate-connect/assets/`**）。

### 2.3 Web API 405（如「删除文件夹」）或接口版本不一致

典型文案：**请求失败 (405)：HTTP 方法不被允许**。

原因：WebView 连到了**旧版** `crabmate serve`（本机残留旧进程，或 URL 指错）。

排查与修复：

1. 确认连接页 / **`CM_DESKTOP_SERVE_URL`** 指向的进程版本与当前仓库一致。
2. 仓库根：**`cargo build`** → **`cd frontend && trunk build`** → 重启 **`serve`** → 再开壳。
3. 前端对 **`DELETE /workspace/dir`** 在 404/405 时会回退 **`POST /workspace/dir`**（**`delete=true`**）。

**预防**：改 **`serve` 路由 / 桌面连接逻辑时，同一 PR 更新本文、`README.md`、`docs/命令行与路由.md` 与（若涉及）**`docs/design/tauri_gui_mvp_design.md`**。

### 2.4 图标/配置错误导致 Tauri 宏报错

典型表现：

- `tauri::generate_context!()` 报 icon/path 相关 panic

排查点：

- `desktop-tauri/src-tauri/tauri.conf.json` 中路径是否存在
- `desktop-tauri/src-tauri/icons/icon.png` 是否为合法 RGBA PNG

### 2.5 工作区嵌套导致 Cargo workspace 报错

如果出现 “current package believes it's in a workspace when it's not”，可保持：

- `desktop-tauri/src-tauri/Cargo.toml` 包含空 `[workspace]`

用于将该子工程作为独立 workspace 处理。

### 2.6 Wayland 下 fcitx5 等 IME 在 WebView 内异常

在 **Wayland** 会话里，GTK/WebKitGTK（Tauri 嵌入页）对输入法支持仍可能不完整。若设置 **`GDK_BACKEND=x11`** 后正常，说明走 **X11（XWayland）** 可规避。

打包的 **deb** 在 `bundle > linux > deb > files` 中**覆盖** `usr/share/applications/crabmate.desktop`（见 `src-tauri/bundle/deb/crabmate.desktop`），在 `Exec=` 中注入 `GDK_BACKEND=x11`。同一段映射里还会把默认配置打入安装包（`/etc/crabmate/…`）。

本地调试可：

```bash
GDK_BACKEND=x11 cargo tauri dev
```

## 3. 代理与网络说明

### 3.1 什么时候需要代理

以下场景通常需要：

- 首次拉取 Tauri/Rust 依赖（访问 crates.io 慢或超时）
- CI 或受限网络环境下构建

### 3.2 设置代理（bash）

```bash
export http_proxy=http://localhost:8118
export https_proxy=http://localhost:8118
```

### 3.3 代理相关故障

常见错误：`Timeout was reached`、crates 下载失败。确认代理可用后重试。

访问本机 `serve` 时建议：

```bash
export no_proxy=127.0.0.1,localhost
```

## 4. 当前实现边界（MVP）

当前桌面端已具备：

- 连接页连接已运行的本机/远程 `serve`（壳不 spawn）
- 单实例保护；第二次启动唤醒已有窗口
- 系统托盘与最小化隐藏；托盘不可用时安全降级
- 启动失败时的错误对话框 / 闪屏错误态

尚待完善：

- 日志目录与诊断页
- 发现局域网内 `serve`（可选）

## 5. 发布检查清单

发布前建议最少检查：

- **前端与壳同次构建**：`cd frontend && trunk build --release`；`bash desktop-tauri/scripts/prepare-sidecar.sh`（同步 `connect`/`splash` 与可选 `frontend/dist`）
- **桌面 `.deb` 不再内嵌 `crabmate` sidecar**；用户需另装 CLI/`serve` 或连远程
- 安装后 **`/usr/share/crabmate/frontend/dist`**（若仍映射进包）可供本机 `serve` 经 **`CM_WEB_STATIC_DIR`** 使用
- 冷启动：先起 `serve`，再开壳，连接页可连通

## 6. 打包（deb）

`tauri.conf.json`：

- `beforeBuildCommand` / `beforeDevCommand` 调用 `desktop-tauri/scripts/prepare-sidecar.sh`
- **无** `bundle.externalBin`
- `bundle.targets` 仅 `deb`

建议：

```bash
cd /path/to/crabmate_agent
cargo build --release   # 若需同机安装 CLI/serve
cd frontend && trunk build --release && cd ..
cd desktop-tauri/src-tauri
cargo tauri build
```
