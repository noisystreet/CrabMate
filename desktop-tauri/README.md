# desktop-tauri

基于 Tauri 2 的 CrabMate 桌面壳：**WebView** 加载本仓库 **`serve`** 提供的 Web UI，业务逻辑不重复实现。

## 启动流程（与代码一致）

1. `src-tauri/src/main.rs` 按优先级解析后端二进制（**`CM_DESKTOP_BACKEND_BIN`** → sidecar → **`PATH`**）。
2. 子进程命令：**`crabmate serve --host 127.0.0.1 --port 0 --desktop-ready-json`**（若存在 **`/etc/crabmate/config.toml`** 则追加 **`--config`**）。
3. 读取 stdout 中 **`{"event":"web_ready",…}`**，取 **`url`** 打开主窗口。
4. 应用退出时 kill 子进程。

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

- **`frontend/dist`** 须已构建；**`serve`** 从仓库根解析该目录。
- 开发时**务必**用 **`CM_DESKTOP_BACKEND_BIN`** 指向刚编译的 **`target/debug/crabmate`**，避免 PATH / 旧 sidecar 版本不一致。

## 打包

见仓库根 **`README.md`**「桌面 Tauri」与 **`DEVELOPMENT.md`** § 6（**`prepare-sidecar.sh`**、**`cargo tauri build`**）。

## 更多

- 故障排查、代理、Wayland IME：**`DEVELOPMENT.md`**
- 架构与 **`web_ready` 字段：** **`docs/design/tauri_gui_mvp_design.md`**
- 用户数据 HTTP 契约（Tauri 与 Web 共用）：**`docs/design/user_data_dir.md`**

主 Web 前端仍在 **`frontend/`**，桌面端只提供壳层与进程管理。
