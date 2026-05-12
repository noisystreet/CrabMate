[
ToolSpec {
            name: "run_command",
            description: "在服务器上执行 Linux 命令。分两种模式：\n\n**1. 白名单系统命令**：command 必须是配置中 allowed_commands 白名单之一（默认可含 **coreutils 类**：stat、grep、sort、diff、find、which…；**系统信息**：ps、free、uptime、lscpu、lsblk…；**压缩流读出**：zcat/bzcat/xzcat；**JSON**：jq；**Git / Rust**：git、gh、cargo、rustc；**编译与 Binutils**：gcc、clang、make、cmake、ctest、mkdir、objdump…；prod 默认无编译链等）。\n\n**2. 工作区脚本/可执行文件**：若 command 形如 `./xxx`、`path/to/script` 等相对路径，且解析到工作区内已存在的可执行文件或常见脚本（sh/bash/py/pl/rb/js/ts 等），会**自动审批通过**（跳过白名单人工确认）。\n\n**参数格式（推荐）**：`command` 仅填**单个**程序名或相对路径，`args` 为参数数组（每项独立，不含程序名）。**兼容**：若误将 `prog arg1 …` 整段写入 `command` 且**不含 `/`**，实现会按空白拆成程序名并把余下词**前插**到 `args` 前（与 `terminal_session` 共用同一套解析）；含 `/` 的路径不做拆分，以免误伤。\n**推荐示例**：\n- command: cat, args: [main.cpp]\n- command: which, args: [gcc, g++]\n- command: ./build/my_app, args: []\n- command: pre-commit, args: [run, --all-files]\n\n**仍应避免**：\n- command: which, args: [which gcc] —— args 不应再含程序名\n- command: ./my_app, args: [build/my_app] —— 路径重复\n\n**禁止**：rm、mv、chmod、sudo 等未列入白名单则不可执行。\n\n【cmake 常用模式】\n1. **推荐：使用 -S 和 -B 参数**（在源目录外配置）：cmake -S . -B build 然后 cmake --build build\n2. **传统方式**：mkdir -p build && cmake ..（在 build 目录内执行 .. 配置源目录）\n3. cmake 产物路径取决于构建配置，常用位置包括 build/bin/<target>、build/<target>，运行前请先用 ls build 确认",
            category: ToolCategory::Development,
            parameters: tool_params::params_run_command,
            runner: runner_run_command,
            summary: ToolSummaryKind::Dynamic(ts::summary_run_command),
        },
ToolSpec {
            name: "terminal_session",
            description: "**Linux 专用**：伪终端（PTY）交互会话。与 **`run_command`** 共用 **`allowed_commands`** 审批与白名单；输出通过 SSE **`tool_output_chunk`** 流式增量下发（最终以 **`tool_result`** 收束正文）。\n\n- **exec**：无 **`session_id`** 时启动新会话（必填 **`command`** + 可选 **`args`**）；已存在会话则写入 **`input`** 并读取一轮输出直至短时静默。\n- **list** / **close** / **resize** / **send_signal**：会话列举、关闭、窗口尺寸、`kill` 信号。\n\n受限：同时活跃会话 ≤8；超时与输出上限继承 **`command_exec`**（`command_timeout_secs`、`command_max_output_len`）。",
            category: ToolCategory::Development,
            parameters: tool_params::params_terminal_session,
            runner: runner_terminal_session,
            summary: ToolSummaryKind::Dynamic(ts::summary_terminal_session),
        },
ToolSpec {
            name: "package_query",
            description: "只读查询 Linux 包信息（apt/rpm 统一抽象）：是否安装、版本、来源。默认 manager=auto（优先 dpkg-query，再尝试 rpm）；不执行安装/卸载操作。",
            category: ToolCategory::Development,
            parameters: tool_params::params_package_query,
            runner: runner_package_query,
            summary: ToolSummaryKind::Dynamic(ts::summary_package_query),
        },
]
