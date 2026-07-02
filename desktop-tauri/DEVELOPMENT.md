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
- 若存在 `/etc/crabmate/config.toml`，自动追加 `--config /etc/crabmate/config.toml`

并等待后端输出 `web_ready` JSON，再加载 WebView URL。

### 1.3 指定后端可执行文件路径

如果系统 PATH 中没有 `crabmate`，请设置环境变量：

```bash
export CM_DESKTOP_BACKEND_BIN=/absolute/path/to/crabmate
```

### 1.4 后端路径解析优先级（env + sidecar + PATH）

桌面端当前按以下顺序尝试拉起后端：

1. `CM_DESKTOP_BACKEND_BIN`（环境变量显式路径）
2. sidecar 常见位置（相对桌面可执行文件目录）：
   - `<exe_dir>/crabmate`（Windows 为 `crabmate.exe`）
   - `<exe_dir>/sidecar/crabmate`
   - `<exe_dir>/resources/sidecar/crabmate`
3. `PATH` 中的 `crabmate`（Windows 为 `crabmate.exe`）

建议：

- 开发环境优先使用 `CM_DESKTOP_BACKEND_BIN`，路径最明确
- 打包发布时优先 sidecar 路径，避免依赖用户 PATH

### 1.5 常用开发命令

在 **`desktop-tauri/src-tauri`** 目录：

```bash
cargo check
CM_DESKTOP_BACKEND_BIN=/absolute/path/to/crabmate cargo tauri dev
```

完整链路（仓库根目录）：

```bash
cargo build
cd frontend && trunk build && cd ..
cd desktop-tauri/src-tauri
CM_DESKTOP_BACKEND_BIN=/absolute/path/to/target/debug/crabmate cargo tauri dev
```

发布构建：**`cargo tauri build`**（**`beforeBuildCommand`** 会执行 **`prepare-sidecar.sh`**）。

## 2. 常见故障

### 2.1 启动时报 “failed to spawn backend”

原因：

- `crabmate` 不在 PATH
- `CM_DESKTOP_BACKEND_BIN` 路径错误
- 可执行权限不足
- sidecar 未随包或位置不符合预期

排查：

```bash
which crabmate
ls -l /absolute/path/to/crabmate
```

并按“路径解析优先级”逐层确认：

1. `echo $CM_DESKTOP_BACKEND_BIN`
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

若 stderr/日志为 **`unexpected argument '--desktop-ready-json' found`**：deb 或 sidecar 里的 **`crabmate` 过旧**，与当前桌面壳不匹配。**无**旧版回退；须按 §6 重编后端并重打 **`cargo tauri build`** 后再 **`dpkg -i`**。

### 2.3 Web API 405（如「删除文件夹」）或接口版本不一致

典型文案：**请求失败 (405)：HTTP 方法不被允许**。

原因：

- WebView 连到了**旧版** `crabmate serve`（常见：曾用固定 **3000** 端口 + TCP 探测，本机已有旧进程在监听）。
- 仅重编前端或仅重编 Tauri，但 **`CM_DESKTOP_BACKEND_BIN` / sidecar** 仍指向旧二进制。
- **`frontend/dist`** 未重建，UI 已调新 API 但静态资源过旧（较少见）。

排查与修复：

1. **完全退出** Tauri（确保子进程被 kill），勿只关窗口。
2. 确认无残留 **`serve`**：`ss -ltnp | rg crabmate` 或 `pgrep -a crabmate`。
3. 仓库根依次：**`cargo build`** → **`cd frontend && trunk build`**。
4. 用 **`CM_DESKTOP_BACKEND_BIN`** 指向 **`target/debug/crabmate`** 再 **`cargo tauri dev`**。
5. 启动日志中必须有 **`web_ready`** JSON；手动验证：
   ```bash
   curl -s -X DELETE "http://127.0.0.1:<port>/workspace/dir?path=test&confirm=true&recursive=true" -w "\n%{http_code}\n"
   ```
   新后端应返回 **200**（JSON **`error`** 可为业务错误，非 405）。
6. 前端对 **`DELETE /workspace/dir`** 在 404/405 时会回退 **`POST /workspace/dir`**（**`delete=true`**）；仍失败则几乎可断定后端过旧。

**预防**：改 **`serve` 路由 / Tauri 启动逻辑时，同一 PR 更新本文、`README.md`、`docs/命令行与路由.md` 与（若涉及）**`docs/design/tauri_gui_mvp_design.md`**。

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

在 **Wayland** 会话里，GTK/WebKitGTK（Tauri 嵌入页）对输入法支持仍可能不完整，表现为候选窗不显示或无法内联输入。若在终端中设置 **`GDK_BACKEND=x11`** 后正常，说明走 **X11（XWayland）后端** 可规避。

打包的 **deb** 在 `bundle > linux > deb > files` 中**覆盖** `usr/share/applications/crabmate.desktop`（见 `src-tauri/bundle/deb/crabmate.desktop`），在 `Exec=` 中注入 `GDK_BACKEND=x11`。该步骤在 Tauri 生成默认桌面文件**之后**执行，不依赖 Handlebars 模板是否生效。若修改 `productName` 或二进制名，请同步该文件名与条目的 `Name` / `Exec` / `Icon`。同一段映射里还会把后端默认配置打入安装包：

- `/etc/crabmate/config.toml`
- `/etc/crabmate/agent_roles.toml`（多角色；与 `config.toml` 同目录，见 `docs/配置说明.md`）
- `/etc/crabmate/*.toml`
- `/etc/crabmate/prompts/*.md`（全局 system 等）
- `/etc/crabmate/config/prompts/*.md`（各命名角色的 `system_prompt_file` 增量）
- `/usr/share/doc/crabmate/config/prompts/*.md`

这样桌面包与根仓库 `cargo deb` 在默认配置资产上保持一致，便于排障与离线查阅。

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

- 启动后端子进程（**`--port 0 --desktop-ready-json`**）
- 解析 **`web_ready`** 并加载动态 URL
- 打开 WebView、退出时回收后端进程
- 启动失败时的阻塞错误对话框
- **启动 splash**：先显示 `splash.html`，后台拉起 sidecar，就绪后打开主窗口
- sidecar 默认 **`RUST_LOG=warn`**（可用环境变量覆盖）

性能相关（发布路径）：

- 后端 **release**：`lto = thin` + `strip`（根 `Cargo.toml` `[profile.release]`）
- 前端 **`trunk build --release`** + `wasm-opt`；`make frontend-release` 会先拉取本地字体、合并 CSS
- **CodeMirror** 仅在进入 IDE 模式时动态加载 `/vendor/ide-codemirror.js`
- **`serve`** 对静态资源启用 **gzip**（`tower-http` `CompressionLayer`）

尚待完善：

- 日志目录与诊断页
- 单实例保护
- sidecar 自动更新
- **`web_ready` 与 `/health` 版本号交叉校验**（可选）

## 5. 发布检查清单（sidecar 与前端一致）

发布前建议最少检查以下项目：

- **后端与前端、桌面壳同次构建**
  - **`cargo build --release`**（或 dev 用 **`cargo build`**）
  - **`cd frontend && trunk build --release`**（或 dev 用 **`trunk build`**）
  - **`bash desktop-tauri/scripts/prepare-sidecar.sh`** 会校验 sidecar 支持 **`serve --desktop-ready-json`**，不支持则**直接失败**（无旧版回退）；并将 **`frontend/dist`** 同步到 **`desktop-tauri/dist`**（deb 安装到 **`/usr/share/crabmate/frontend/dist/`**，供 IDE 编辑器 **`vendor/ide-codemirror.js`** 等）
  - 复制的 sidecar 须与上一步 **`crabmate`** 为同一构建产物
- **桌面 `.deb` 须整体重装**
  - 仅 **`dpkg -i`** 新 deb、但 deb 内 sidecar 仍是旧 **`crabmate`** 时，启动会报 **`unexpected argument '--desktop-ready-json'`**；须按 §6 顺序重打包含新 sidecar 的包
- **后端二进制命名**
  - Linux/macOS: `crabmate`
  - Windows: `crabmate.exe`
- **三平台都能命中同一套回退顺序**
  - env：`CM_DESKTOP_BACKEND_BIN`
  - sidecar：`<exe_dir>/crabmate`、`<exe_dir>/sidecar/crabmate`、`<exe_dir>/resources/sidecar/crabmate`
  - PATH：`crabmate` / `crabmate.exe`
- **打包产物内存在 sidecar 文件**
  - 实际解包后确认可执行文件存在且有执行权限（Linux/macOS）
- **冷启动验证**
  - 在“未设置 env、PATH 不含 crabmate”的干净机器上，仍能从 sidecar 启动成功
  - 安装 deb 后 **`/usr/share/crabmate/frontend/dist/index.html`** 与 **`vendor/ide-codemirror.js`** 存在；IDE 模式打开文件编辑区非空
- sidecar 工作目录为可写 **`$HOME`**（会话库 **`.crabmate/conversations.db`**）；静态 UI 经 **`CM_WEB_STATIC_DIR`** 指向 **`/usr/share/crabmate/frontend/dist`**，**勿**把 **`CM_DESKTOP_WORKDIR`** 设为 **`/usr/share/crabmate`**
- **`cargo tauri dev`**（源码树可识别时）将 **`CM_WEB_STATIC_DIR`** 设为仓库 **`frontend/dist`**（已 **`trunk build`** 时），或清除该变量以免继承 shell/deb 的 **`/usr/share/…`** 路径
- **错误提示验证**
  - 故意移除 sidecar 后，能看到明确的启动失败弹窗与路径排查信息

## 6. 打包 sidecar（deb）

`tauri.conf.json` 已配置：

- `beforeBuildCommand` / `beforeDevCommand` 调用 `desktop-tauri/scripts/prepare-sidecar.sh`
- `bundle.externalBin` 为 `../binaries/crabmate`
- `bundle.targets` 仅 `deb`

脚本行为：

1. 优先读取 `CM_DESKTOP_BACKEND_BIN`
2. 未设置时回退到 `<repo>/target/release/crabmate`
3. 复制为 `desktop-tauri/binaries/crabmate-<host-target-triple>`
4. 同步 **`frontend/dist` → `desktop-tauri/dist`**（`tauri.conf.json` **`deb.files`** 安装到 **`/usr/share/crabmate/frontend/dist/`**）

建议打包命令：

```bash
cd /path/to/crabmate_agent
cargo build --release
cd desktop-tauri/src-tauri
cargo tauri build
```
