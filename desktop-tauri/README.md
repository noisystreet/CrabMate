# desktop-tauri

基于 Tauri 2 的 CrabMate 桌面壳：**WebView** 加载已运行的 **`serve`**（本机或远程）提供的 Web UI，业务逻辑不重复实现。**壳不拉起** `crabmate serve`（与移动端同模型）。

## 启动流程（与代码一致）

1. 显示无边框 **闪屏**（`splash.html`），随后打开主窗口。
2. **默认**：主窗口展示与移动端共用的**连接页**（`crates/crabmate-connect/assets/connect.html`）。预填建议地址为 **`CM_DESKTOP_SUGGESTED_URL`**，未设时为 **`http://127.0.0.1:8080/`**。用户填写 **服务器地址** + 可选 **Web API Bearer**，探测 `GET /health` 成功后导航到该 `serve` UI，并用 hash 交接 Bearer。
3. **首次**成功连接且 Bearer 非空、本机钥匙串尚无对应条目时，写入系统钥匙串（账户 `tauri_connect_web_api_bearer`）；下次启动自动填充。
4. 桌面应用保持单实例：再次启动会显示并聚焦已有主窗口（启动中则聚焦闪屏）。
5. 关闭主窗口会结束应用；系统托盘可用时，最小化按钮会隐藏主窗口，托盘「显示/隐藏」可恢复。托盘初始化失败时保留普通最小化。
6. 主窗口退出时保存大小、位置与最大化状态，下次启动恢复；启动闪屏不参与状态保存。右侧可拖拽分栏宽度沿用 Web 偏好持久化。

**跳过连接页**（直接打开指定 URL）：**`CM_E2E_FIXTURES=1`**（Victauri E2E）或 **`CM_DESKTOP_SKIP_CONNECT=1`**，且必须设置 **`CM_DESKTOP_SERVE_URL`**（例如 `http://127.0.0.1:8080/`）。须事先自行启动 `serve`。

闪屏文案提示「请先自行启动 serve」；失败时在闪屏内展示错误与「退出」。主窗首屏 page load 完成（或约 20s 超时兜底）后再显示主窗并关闭闪屏。

## 托盘与单实例

- 托盘右键菜单提供「显示/隐藏」「退出」；Windows/macOS 还可用左键切换。Linux 以菜单为准。
- 点击窗口关闭按钮会退出应用；需要让本机 `serve` 继续运行时，请使用最小化隐藏到托盘（壳不管理 `serve` 进程生命周期）。
- 第二次启动不会再开第二个壳窗口；已有窗口会显示并获得焦点。

请先在本机或远程启动 **`crabmate serve`**，再开桌面壳连接；勿假设壳会自动起后端。

## 本地开发

### 前置

- Rust stable、Tauri 2 系统依赖
- **`cargo install tauri-cli --version "^2"`**（一次性）
- 另开终端运行 **`cargo run -- serve`**（或已有远程 `serve`）

### 推荐步骤（仓库根目录）

```bash
cargo build
cd frontend && trunk build && cd ..
cargo run -- serve   # 另开终端；默认 http://127.0.0.1:8080/

cd desktop-tauri/src-tauri
cargo tauri dev
```

- **`prepare-sidecar.sh`**（名称历史遗留）会把 **`connect.html`** / **`splash.html`** 拷进 **`desktop-tauri/dist/`**，并可选同步 **`frontend/dist`**（deb 仍可安装静态资源，供本机另装的 `serve` 使用 **`CM_WEB_STATIC_DIR`**）。
- 可选：**`CM_DESKTOP_SUGGESTED_URL`** 覆盖连接页预填。

## 打包

见仓库根 **`README.md`**「桌面 Tauri」与 **`DEVELOPMENT.md`**（**`prepare-sidecar.sh`**、**`cargo tauri build`**）。桌面包**不再**内嵌 `crabmate` sidecar 二进制。

## 更多

- 故障排查、代理、Wayland IME：**`DEVELOPMENT.md`**
- 架构：**`docs/design/tauri_gui_mvp_design.md`**
- 用户数据 HTTP 契约：**`docs/design/user_data_dir.md`**
- 共用连接逻辑：**`crates/crabmate-connect`**（与 **`mobile-tauri`** 对齐；crate README 与 **`docs/design/client_contract_versioning.md`** 说明 **git tag / Tauri 2** 钉法）
- 官方 Client 拆分（路径 A）：**`docs/design/client_shell_split.md`**

主 Web 前端仍在 **`frontend/`**，桌面端只提供壳层与连接页。
