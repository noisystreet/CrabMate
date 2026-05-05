# desktop-tauri 开发说明

本文面向本仓库的桌面端开发者，说明如何在本地运行 `desktop-tauri`，以及常见故障和代理/网络配置建议。

## 1. 本地运行

### 1.1 前置依赖

- Rust 工具链（建议 stable）
- Tauri 2 所需系统依赖（按官方文档安装）
- 本仓库后端可执行文件（`crabmate`）

### 1.2 启动方式（当前实现）

桌面端位于：

- `desktop-tauri/src-tauri`

当前桌面壳逻辑会在启动时执行：

- `crabmate serve --host 127.0.0.1 --port 0 --desktop-ready-json`

并等待后端输出 `web_ready` JSON，再加载 WebView URL。

### 1.3 指定后端可执行文件路径

如果系统 PATH 中没有 `crabmate`，请设置环境变量：

```bash
export CRABMATE_DESKTOP_BACKEND_BIN=/absolute/path/to/crabmate
```

### 1.4 后端路径解析优先级（env + sidecar + PATH）

桌面端当前按以下顺序尝试拉起后端：

1. `CRABMATE_DESKTOP_BACKEND_BIN`（环境变量显式路径）
2. sidecar 常见位置（相对桌面可执行文件目录）：
   - `<exe_dir>/crabmate`（Windows 为 `crabmate.exe`）
   - `<exe_dir>/sidecar/crabmate`
   - `<exe_dir>/resources/sidecar/crabmate`
3. `PATH` 中的 `crabmate`（Windows 为 `crabmate.exe`）

建议：

- 开发环境优先使用 `CRABMATE_DESKTOP_BACKEND_BIN`，路径最明确
- 打包发布时优先 sidecar 路径，避免依赖用户 PATH

### 1.5 常用开发命令

在 `desktop-tauri/src-tauri` 目录下执行：

```bash
cargo check
```

> 说明：如果只做 Rust 侧快速校验，`cargo check` 即可。  
> 运行/打包命令可在后续完善阶段补齐（例如 `cargo tauri dev` / `cargo tauri build`）。

## 2. 常见故障

### 2.1 启动时报 “failed to spawn backend”

原因：

- `crabmate` 不在 PATH
- `CRABMATE_DESKTOP_BACKEND_BIN` 路径错误
- 可执行权限不足
- sidecar 未随包或位置不符合预期

排查：

```bash
which crabmate
ls -l /absolute/path/to/crabmate
```

并按“路径解析优先级”逐层确认：

1. `echo $CRABMATE_DESKTOP_BACKEND_BIN`
2. 检查 sidecar 位置是否存在后端可执行文件
3. 再检查 PATH 里的 `crabmate`

### 2.2 一直等待 ready 或超时

原因：

- 后端未成功启动
- 后端未输出 `web_ready` 行
- 本机端口被安全策略拦截

排查建议：

1. 手动运行后端命令，确认会输出 ready JSON：

```bash
crabmate serve --host 127.0.0.1 --port 0 --desktop-ready-json
```

2. 检查日志中是否出现：

- `{"event":"web_ready", ...}`

### 2.3 图标/配置错误导致 Tauri 宏报错

典型表现：

- `tauri::generate_context!()` 报 icon/path 相关 panic

排查点：

- `desktop-tauri/src-tauri/tauri.conf.json` 中路径是否存在
- `desktop-tauri/src-tauri/icons/icon.png` 是否为合法 RGBA PNG

### 2.4 工作区嵌套导致 Cargo workspace 报错

如果出现 “current package believes it's in a workspace when it's not”，可保持：

- `desktop-tauri/src-tauri/Cargo.toml` 包含空 `[workspace]`

用于将该子工程作为独立 workspace 处理。

### 2.5 Wayland 下 fcitx5 等 IME 在 WebView 内异常

在 **Wayland** 会话里，GTK/WebKitGTK（Tauri 嵌入页）对输入法支持仍可能不完整，表现为候选窗不显示或无法内联输入。若在终端中设置 **`GDK_BACKEND=x11`** 后正常，说明走 **X11（XWayland）后端** 可规避。

打包的 **deb** 在 `bundle > linux > deb > files` 中**覆盖** `usr/share/applications/crabmate.desktop`（见 `src-tauri/bundle/deb/crabmate.desktop`），在 `Exec=` 中注入 `GDK_BACKEND=x11`。该步骤在 Tauri 生成默认桌面文件**之后**执行，不依赖 Handlebars 模板是否生效。若修改 `productName` 或二进制名，请同步该文件名与条目的 `Name` / `Exec` / `Icon`。

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

可按命令级使用：

```bash
export http_proxy=http://localhost:8118 && export https_proxy=http://localhost:8118 && cargo check
```

### 3.3 代理相关故障

常见错误：

- `Timeout was reached`
- crates 下载失败

建议：

1. 确认代理服务可用（本地 `localhost:8118` 正常监听）
2. 先在 shell 中 `echo $http_proxy` / `echo $https_proxy` 确认变量生效
3. 失败后重试一次（网络抖动时有效）

## 4. 当前实现边界（MVP）

当前桌面端已具备：

- 启动后端进程
- 解析 `web_ready`
- 打开 WebView
- 退出时回收后端进程

尚待完善：

- 桌面启动失败的可视化提示
- 日志目录与诊断页
- 单实例保护
- sidecar 自动更新

## 5. 发布检查清单（sidecar 路径一致性）

发布前建议最少检查以下项目：

- **后端二进制命名**
  - Linux/macOS: `crabmate`
  - Windows: `crabmate.exe`
- **三平台都能命中同一套回退顺序**
  - env：`CRABMATE_DESKTOP_BACKEND_BIN`
  - sidecar：`<exe_dir>/crabmate`、`<exe_dir>/sidecar/crabmate`、`<exe_dir>/resources/sidecar/crabmate`
  - PATH：`crabmate` / `crabmate.exe`
- **打包产物内存在 sidecar 文件**
  - 实际解包后确认可执行文件存在且有执行权限（Linux/macOS）
- **冷启动验证**
  - 在“未设置 env、PATH 不含 crabmate”的干净机器上，仍能从 sidecar 启动成功
- **错误提示验证**
  - 故意移除 sidecar 后，能看到明确的启动失败弹窗与路径排查信息

## 6. 打包 sidecar（deb）

`tauri.conf.json` 已配置：

- `beforeBuildCommand` / `beforeDevCommand` 调用 `desktop-tauri/scripts/prepare-sidecar.sh`
- `bundle.externalBin` 为 `../binaries/crabmate`
- `bundle.targets` 仅 `deb`

脚本行为：

1. 优先读取 `CRABMATE_DESKTOP_BACKEND_BIN`
2. 未设置时回退到 `<repo>/target/release/crabmate`
3. 复制为 `desktop-tauri/binaries/crabmate-<host-target-triple>`

建议打包命令：

```bash
cd /path/to/crabmate_agent
cargo build --release
cd desktop-tauri/src-tauri
cargo tauri build
```
