[
ToolSpec {
            name: "run_command",
            description: "在服务器上执行 Linux 命令（白名单 + 工作区路径安全）。\n\n**1. 基本用法**：`command` 填单个程序名或相对路径，`args` 为参数数组（每项独立，不含程序名）；command 须在 `allowed_commands` 白名单内，或为工作区内已存在的可执行文件/脚本（自动审批）。白名单含 `bash/sh` 时，含 glob/`$VAR`/`&&`/管道的 argv 会整行经 `bash -c` 执行（审批展示完整脚本；反引号与 `$()` 命令替换按字面量转义、不会执行，需要时请用 `script`）。\n**推荐**：`cat [main.cpp]`、`which [gcc,g++]`、`./build/my_app []`。\n**避免**：`which [which gcc]`（args 重复程序名）、`./my_app [build/my_app]`（路径重复）、未白名单的 `rm/mv/chmod/sudo`。\n\n**2. 路径与 cwd**：优先工作区相对路径；可选 `cwd` 指定相对工作目录，不得为绝对路径或含 `..`。argv 含工作区外绝对路径或 `..` 路径分量时需人工审批（`main..HEAD` 不算；`cmake ..` 为合法特例，推荐 `cmake -S . -B build`）。\n\n**3. shell 脚本**：可选 `script` 用一行 shell 脚本代替 `command`+`args`（二者互斥）；白名单无 `bash/sh` 时报错，有 shell 但未获信任时需审批；脚本正文内的路径不由本工具审计。\n\n**4. 异步与超时**：`timeout_secs` 墙钟秒（钳制 1～600）；`async: true` 转后台任务并立即返回 `tool_job_id`，之后用 `background_job`（`action=list` / `status`）查询，需开启 `[tool_registry] background_jobs_enabled`，命令须已在白名单或已 AllowAlways 批准。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::RunCommandArgs>,
            runner: ToolRunner::AsyncOnly,
            summary: ToolSummaryKind::Dynamic(ts::summary_run_command),
        },
ToolSpec {
            name: "terminal_session",
            description: "**Linux 专用**：伪终端（PTY）交互会话。与 **`run_command`** 共用 **`allowed_commands`**、路径安全与（可选）工作区外路径人工审批；输出通过 SSE **`tool_output_chunk`** 流式增量下发（最终以 **`tool_result`** 收束正文）。\n\n- **exec**：无 **`session_id`** 时启动新会话（必填 **`command`** + 可选 **`args`**）；已存在会话则写入 **`input`** 并读取一轮输出直至短时静默。\n- **list** / **close** / **resize** / **send_signal** / **interrupt**：会话列举、关闭、窗口尺寸、`kill` 信号；**interrupt** 向会话前台进程组写 ETX（等价 Ctrl-C，优先用于中断前台任务），**send_signal** 默认发给前台进程组，无前台进程组时回落到会话首进程。\n\n受限：同时活跃会话 ≤8；输出上限继承 **`command_exec`**（`command_max_output_len`）。单轮读取的墙钟为 `command_timeout_secs`，到点仅结束本轮读取并**保留会话**以便后续 `exec` 继续交互；仅当**上层工具调用被取消**或**外层执行超时**时才回收会话，避免占用 8 路上限；会话期间的增量输出随 `tool_output_chunk` 下发。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::TerminalSessionArgs>,
            runner: ToolRunner::AsyncOnly,
            summary: ToolSummaryKind::Dynamic(ts::summary_terminal_session),
        },
]
