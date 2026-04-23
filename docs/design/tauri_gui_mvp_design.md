# 基于现有 Web UI 的 Tauri GUI 设计（MVP）

## 1. 目标

在不重构现有业务逻辑的前提下，为项目新增一个桌面 GUI：

- 复用已有 `frontend-leptos` Web UI
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

### 3.1 后端（本阶段）

在 CLI `serve` 子命令新增桌面握手开关：

- 新参数：`--desktop-ready-json`
- 行为：当服务监听成功后，额外打印一行 JSON：

```json
{"event":"web_ready","host":"127.0.0.1","port":8080,"url":"http://127.0.0.1:8080/","auth_enabled":false}
```

说明：

- 该输出仅在显式开启 `--desktop-ready-json` 时出现
- 为避免端口占用误判，应在 `TcpListener::bind` 成功后基于 `local_addr()` 生成
- 支持 `--port 0` 随机端口场景

### 3.2 桌面端（本阶段）

新增 `desktop-tauri/` 最小骨架，用于承载后续实现：

- `desktop-tauri/src-tauri/Cargo.toml`
- `desktop-tauri/src-tauri/tauri.conf.json`
- `desktop-tauri/src-tauri/src/main.rs`
- `desktop-tauri/src-tauri/build.rs`
- `desktop-tauri/README.md`

本阶段先提供“可扩展骨架”，下一阶段接入真实后端子进程与 URL 动态加载。

## 4. 实施步骤（MVP）

1. 后端新增 `--desktop-ready-json` 参数与 ready 输出
2. 增加基础文档与启动示例
3. 建立 Tauri 工程骨架（先不耦合具体启动脚本）
4. 下一阶段完成：
   - 启动 `crabmate serve --host 127.0.0.1 --port 0 --desktop-ready-json`
   - 解析 ready JSON
   - 加载动态 URL
   - 退出时回收子进程

### 开发启动命令（当前实现）

1. 在仓库根目录编译后端：

```bash
cd /home/gzz/code/agent_demo
cargo build
```

2. 启动 Tauri 开发界面：

```bash
cd /home/gzz/code/agent_demo/desktop-tauri/src-tauri
CRABMATE_DESKTOP_BACKEND_BIN=/home/gzz/code/agent_demo/target/debug/crabmate cargo tauri dev
```

3. 若未安装 Tauri CLI，先安装：

```bash
cargo install tauri-cli --version "^2"
```

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
