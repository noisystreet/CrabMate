# CrabMate

CrabMate 是一个基于 **DeepSeek API** 从零实现的简易 Rust AI Agent，支持**工具调用**（Function Calling），能在工作区内执行命令、查看/编辑文件并给出自然语言回复。

## 功能概览

- **调用 DeepSeek 对话接口**，支持多模型切换（见下方配置）。
- **内置多种工具，由模型按需调用**：
  - `get_current_time`：获取当前日期时间。
  - `calc`：使用 Linux 的 `bc -l` 执行数学表达式（四则、乘方 ^、sqrt/sin/cos/tan/ln/exp、pi/e 等）。
  - `get_weather`：获取指定城市/地区当前天气（[Open-Meteo](https://open-meteo.com/) API，无需 Key）。
  - `run_command`：执行白名单内的只读/查询类 Linux 命令（`ls`、`pwd`、`whoami`、`date`、`cat`、`head`、`tail`、`wc`、`cmake`、`gcc`、`g++`、`make` 等），带超时与输出截断。
  - `run_executable`：在工作区目录下运行可执行文件（路径、参数均做安全校验）。
  - `create_file` / `modify_file`：在当前工作区内创建或修改文件（相对路径 + 目录越界检查）。
- **工作区浏览与文件编辑**（Web UI 右侧面板）：
  - 浏览当前工作目录的文件/子目录。
  - 在前端新建/编辑文件，保存后自动刷新工作区列表。
  - Agent 通过工具创建/修改文件后，前端会自动检测并刷新工作区。
- **命令执行与结果展示**：
  - Agent 下发 `run_command` / `run_executable` 时，前端会显示一条“系统消息”摘要（例如 `执行命令：g++ main.cpp -o main`）。
  - 命令执行完成后，命令输出（stdout/stderr、退出码）会以单独的系统气泡展示在聊天框中，便于直接查看 `ls`、编译日志等。
- **流式输出与状态栏**：
  - Chat 回复支持流式增量显示。
  - 状态栏区分“模型生成中…”和“工具运行中…”，命令完成后不会一直显示忙碌。
- **会话保存**：
  - 顶部菜单栏提供“保存会话”按钮，可将当前对话导出为 JSON 文件，便于归档或调试。

## 环境

- Rust 1.70+
- 环境变量：`API_KEY`，值为 [DeepSeek 开放平台](https://platform.deepseek.com/) 的 API Key

## 配置与多模型切换

**默认配置**来自项目根目录的 `default_config.toml`（含 `api_base`、`model`）。可在当前工作目录用 `config.toml` 或 `.agent_demo.toml` 覆盖，再被环境变量覆盖（为了兼容早期命名，保留 `.agent_demo.toml` 作为别名）。

1. **环境变量**（优先级最高）  
   - `AGENT_API_BASE`：API 基础 URL  
   - `AGENT_MODEL`：模型 ID  
   - `AGENT_SYSTEM_PROMPT`：系统提示词（内联）  
   - `AGENT_SYSTEM_PROMPT_FILE`：系统提示词文件路径（与上二选一，文件优先）  
   ```bash
   export AGENT_MODEL=deepseek-reasoner
   cargo run
   ```
2. **配置文件**：`config.toml` 或 `.agent_demo.toml`（可只写要覆盖的项）：
   ```toml
   [agent]
   api_base = "https://api.deepseek.com/v1"
   model = "deepseek-reasoner"
   # 系统提示词：内联或从文件加载
   # system_prompt = "你是专业的助手。"
   # system_prompt_file = "system_prompt.txt"
   ```
   可参考 `config.toml.example`。

**系统提示词**：在 `default_config.toml` 中通过 `system_prompt`（多行字符串）或 `system_prompt_file`（文件路径）配置；若同时设置，以文件内容为准。未配置则启动报错。

常用模型 ID：`deepseek-chat`（默认）、`deepseek-reasoner`（推理链更长，适合复杂推理）。

## 编译与运行（命令行选项）

基础运行方式：

```bash
export API_KEY="your-api-key"
cargo run
```

### 常用命令行选项

CrabMate 支持几种常见运行模式，对应 `src/main.rs` 中的 CLI 解析：

| 选项              | 作用 |
|-------------------|------|
| `-h, --help`      | 显示命令行帮助与示例。|
| `--config <path>` | 显式指定配置文件路径。指定后仅从该文件合并配置，不再查找当前目录下的 `config.toml` / `.agent_demo.toml`。|
| `--serve [port]`  | 以 Web 服务模式启动，默认端口 `8080`。可传入端口号，如 `--serve 3000`。|
| `--query <问题>`  | 单次提问模式：命令行参数中直接给出问题，输出回答后进程退出，适合脚本调用。|
| `--stdin`         | 管道模式：从标准输入读取问题（多行直到 EOF），输出回答后退出，适合 `echo ... | crabmate --stdin` 这种用法。|
| `--workspace <path>` | 启动时指定初始工作区路径（覆盖配置中的 `run_command_working_dir`，仅当前进程生效）。|
| `--output <mode>` | 仅对 `--query` / `--stdin` 生效；`plain` 为默认，`json` 会在末尾额外输出一行 JSON 结果。|
| `--no-tools`      | 禁用所有工具调用，仅作为普通 Chat 使用。|
| `--no-web`        | 仅提供后端 API，不挂载前端静态页面（适合部署为纯后端服务）。|
| `--cli-only`      | 等价于 `--no-web`，便于按习惯书写。|
| `--dry-run`       | 仅检查配置是否可加载、`API_KEY` 是否存在以及前端静态目录是否存在，然后退出，可用于 CI 自检。|
| `--no-stream`     | 在命令行模式下关闭流式输出，等待完整回答后一次性打印。|

对应示例：

```bash
# 使用默认配置交互运行
cargo run

# 使用指定配置文件（覆盖默认 config.toml / .agent_demo.toml 搜索）
cargo run -- --config /path/to/my.toml

# Web 服务模式（默认 8080）
cargo run -- --serve

# Web 服务模式（指定端口）
cargo run -- --serve 3000

# Web 服务模式并指定初始工作区
cargo run -- --serve 8080 --workspace /path/to/project

# 单次提问
cargo run -- --query "北京今天天气怎么样"

# 单次提问并以 JSON 结果形式返回（便于脚本消费）
cargo run -- --output json --query "北京今天天气怎么样"

# 从标准输入读入问题（多行直到 EOF）
echo "1+1等于几" | cargo run -- --stdin

# 禁用所有工具，仅使用模型本身
cargo run -- --no-tools --serve
```

前端在 **`frontend/`** 目录（Vite + React + TypeScript + Tailwind CSS），需先构建后启动后端：

```bash
cd frontend && npm install && npm run build && cd ..
cargo run -- --serve
```

后端从 `frontend/dist` 提供静态页面，API 与页面同源，无需 CORS。

- **GET /**：前端页面（聊天 + 工作区 + 状态栏），在浏览器打开即可对话。
- **POST /chat**：请求体 `{"message": "你的问题"}`，返回 `{"reply": "助手回复"}`（会走完整 Agent 与工具调用）。
- **GET /status**：返回当前模型、API 地址等后台状态。
- **GET /workspace**：返回当前工作目录路径及文件列表。
- **GET /health**：健康检查，返回 `{"status": "ok"}`。

**单次提问（脚本/管道）**：使用 `--query <问题>` 或 `--stdin` 时，程序只执行一次提问并输出回答后退出，便于在脚本或管道中调用：

```bash
# 参数传入问题
cargo run -- --query "北京今天天气怎么样"

# 从标准输入读入问题（多行直到 EOF）
echo "1+1等于几" | cargo run -- --stdin
```

运行后（交互模式）输入问题，例如：

- 「现在几点？」
- 「(123 + 456) * 2 等于多少？」
- 「北京今天天气怎么样？」
- 「今天几号？再帮我算 100 除以 5」

输入 `quit` / `exit` 或按 **Ctrl+D** 退出。

## 打包为 Debian `.deb` 包

本项目已内置 `cargo-deb` 的打包元数据，可在 Debian/Ubuntu 上打成 `.deb` 包后安装运行。

1. **安装 `cargo-deb` 子命令**（只需一次）：

   ```bash
   cargo install cargo-deb
   ```

2. **构建前端静态资源**（用于 Web 界面）：

   ```bash
   cd frontend
   npm install
   npm run build
   cd ..
   ```

3. **编译后端 Release 二进制**：

   ```bash
   cargo build --release
   ```

4. **生成 `.deb` 安装包**：

   ```bash
   cargo deb
   ```

   生成的安装包位于：

   ```bash
   ls target/debian/*.deb
   ```

5. **在系统中安装与卸载**：

   ```bash
   # 安装
   sudo dpkg -i target/debian/crabmate_0.1.0_amd64.deb

   # 如需卸载
   sudo apt remove crabmate
   ```

安装后可直接运行：

```bash
export API_KEY="your-api-key"
crabmate --serve 8080
```

## 项目结构

```
crabmate/
├── Cargo.toml
├── README.md
├── default_config.toml  # 默认 api_base、model
├── config.toml.example # 配置示例（可选覆盖）
├── frontend/              # Web 前端（Vite + React + TS + Tailwind）
│   └── src/
│       ├── App.tsx        # 布局：聊天 + 工作区 + 状态栏
│       ├── components/    # ChatPanel、WorkspacePanel、StatusBar 等
│       ├── api.ts         # 前端调用 /chat、/chat/stream、/workspace 等 API
│       └── types.ts
└── src/
    ├── main.rs            # 入口、REPL、Web 服务、Agent 主循环
    ├── config.rs          # 配置加载（模型等），环境变量 + 配置文件
    ├── types.rs           # API/消息类型与常量
    ├── api.rs             # DeepSeek 流式请求与 SSE 解析
    └── tools/             # 工具目录，按工具分文件便于扩展
        ├── mod.rs         # 工具列表与 run_tool 分发
        ├── time.rs        # 获取当前时间
        ├── calc.rs        # 数学计算（bc -l）
        ├── weather.rs     # 天气（Open-Meteo）
        ├── command.rs     # 有限 Linux 命令执行（白名单 + 超时）
        ├── exec.rs        # 运行工作区内可执行文件
        ├── file.rs        # 创建 / 修改工作区文件
        ├── grep.rs        # （可选）grep 相关工具
        └── format.rs      # 代码格式化工具（rustfmt / prettier）
```

## 实现要点

- **消息格式**：与 OpenAI Chat Completions 兼容，使用 `messages` + `tools` / `tool_choice`
- **Agent 循环**：若返回 `finish_reason == "tool_calls"`，则本地执行对应工具，将结果以 `role: "tool"` 追加到 `messages`，再次请求 API，直到模型返回普通文本
- **工具执行**：`get_current_time` 用 `chrono`；`calc` 通过 **bc -l**（stdin 传参、不经过 shell）做数学求值，依赖系统已安装 `bc`

## 还可完善的方向

可从以下方向继续增强（按需实现）：

| 方向 | 说明 |
|------|------|
| **会话持久化** | 将对话历史保存到文件，下次启动可加载或续聊 |
| **配置外部化** | 通过环境变量或配置文件设置 `max_tokens`、`temperature`、白名单命令等 |
| **更多工具** | 如：读文件（受限路径）、搜索文件内容、当前目录下的 grep 等 |
| **安全** | run_command 可加「允许的工作目录」限制；或通过环境变量扩展白名单 |
| **日志与调试** | 可选记录请求/响应或仅工具调用，便于排查问题 |
| **代码结构** | 拆成多模块（如 `api.rs`、`tools.rs`）并为主流程和工具写单元测试 |

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
