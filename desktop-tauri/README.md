# desktop-tauri（MVP 骨架）

该目录用于承载基于 Tauri 的桌面 GUI。

当前阶段目标：

- 建立最小 Tauri 工程骨架
- 拉起 `crabmate serve`、解析 ready JSON 并动态加载 Web UI

## 下一步集成计划

当前已实现：

1. `src-tauri/src/main.rs` 启动后端子进程：
   - `crabmate serve --host 127.0.0.1 --port 0 --desktop-ready-json`
2. 读取 stdout 中 `{"event":"web_ready", ...}` 并解析 URL
3. 创建主窗口并加载该 URL
4. 应用退出时回收后端子进程

## 运行方式（开发）

默认会执行 `crabmate` 命令启动后端。若命令不在 PATH，可设置：

`CRABMATE_DESKTOP_BACKEND_BIN=/path/to/crabmate`

推荐的本地开发启动步骤：

1. 在仓库根目录编译后端二进制：

```bash
cd /home/gzz/code/agent_demo
cargo build
```

2. 在 Tauri 目录启动桌面界面（显式指定后端可执行文件路径）：

```bash
cd /home/gzz/code/agent_demo/desktop-tauri/src-tauri
CRABMATE_DESKTOP_BACKEND_BIN=/home/gzz/code/agent_demo/target/debug/crabmate cargo tauri dev
```

若本机尚未安装 Tauri CLI，可先执行：

```bash
cargo install tauri-cli --version "^2"
```

## 备注

当前仓库主 Web 前端仍在 `frontend-leptos/`，桌面端复用该 UI，不重复实现业务页面。
