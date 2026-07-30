# desktop-tauri

基于 Tauri 2 的 CrabMate 桌面壳：**WebView** 加载本仓库 **`serve`** 提供的 Web UI，业务逻辑不重复实现。

## 启动流程（与代码一致）

1. `src-tauri/src/main.rs` 按优先级解析后端二进制（**`CM_DESKTOP_BACKEND_BIN`** → sidecar → **`PATH`**）。
2. 子进程命令：**`crabmate serve --host 127.0.0.1 --port 0 --desktop-ready-json`**（若存在 **`$XDG_CONFIG_HOME/crabmate/config.toml`** 则追加 **`--config`**；可由 **`/etc/crabmate`** 首次种子；种子失败且尚无用户副本时只读回退 **`/etc`**）。
3. 读取 stdout 中 **`{"event":"web_ready",…}`**，取 **`url`** 打开主窗口。
4. 桌面应用保持单实例：再次启动会显示并聚焦已有主窗口（启动中则聚焦闪屏）。
5. 关闭主窗口会结束应用并 kill 子进程；系统托盘可用时，最小化按钮会隐藏主窗口，托盘「显示/隐藏」可恢复。托盘初始化失败时保留普通最小化。
6. 主窗口退出时保存大小、位置与最大化状态，下次启动恢复；启动闪屏不参与状态保存。右侧可拖拽分栏宽度沿用 Web 偏好持久化，恢复时会按当前视口安全夹取。

启动过程中会先显示无边框 **闪屏**（`splash.html`）：进度文案随后端拉起 / `web_ready` 更新；失败时在闪屏内展示错误与「退出」，避免空白窗口。

## 托盘与单实例

- 托盘右键菜单提供「显示/隐藏」「退出」；Windows/macOS 还可用左键切换。Tauri 在 Linux 不派发托盘图标点击事件，因此 Linux 以菜单操作为准。
- 点击窗口关闭按钮会退出应用并结束正在运行的 Agent 回合；需要让任务继续运行时，请使用最小化按钮隐藏到托盘。
- 桌面环境没有可用托盘实现时，启动会记录降级日志，最小化按钮仍执行普通窗口最小化。
- 第二次启动不会再拉起后端；已有窗口会显示、取消最小化并获得焦点。

**勿**在开发机上长期占用固定端口（如 3000）跑独立 **`serve`** 后再开 Tauri；旧实现曾用 TCP 探测固定端口，会误连旧进程并出现 API **405**（例如工作区删目录）。详见 **`DEVELOPMENT.md`** § 2.3。

## 本地开发

### 前置

- Rust stable、Tauri 2 系统依赖
- **`cargo install tauri-cli --version "^2"`**（一次性）

### 推荐步骤（仓库根目录）

```bash
cargo build
cd frontend && trunk build && cd ..

cd desktop-tauri/src-tauri
CM_DESKTOP_BACKEND_BIN=/绝对路径/到/crabmate_agent/target/debug/crabmate cargo tauri dev
```

- **`frontend/dist`** 须已构建；**`serve`** 从仓库根解析该目录（桌面 **`.deb`** 安装后从 **`/usr/share/crabmate/frontend/dist`**，见 **`prepare-sidecar.sh`**）。
- 开发时**务必**用 **`CM_DESKTOP_BACKEND_BIN`** 指向刚编译的 **`target/debug/crabmate`**，避免 PATH / 旧 sidecar 版本不一致。

## 打包

见仓库根 **`README.md`**「桌面 Tauri」与 **`DEVELOPMENT.md`** § 6（**`prepare-sidecar.sh`**、**`cargo tauri build`**）。

## 更多

- 故障排查、代理、Wayland IME：**`DEVELOPMENT.md`**
- 架构与 **`web_ready` 字段：** **`docs/design/tauri_gui_mvp_design.md`**
- 用户数据 HTTP 契约（Tauri 与 Web 共用）：**`docs/design/user_data_dir.md`**

主 Web 前端仍在 **`frontend/`**，桌面端只提供壳层与进程管理。
