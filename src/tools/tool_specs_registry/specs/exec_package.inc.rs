[
ToolSpec {
            name: "run_command",
            description: "在服务器上执行**白名单内**的 Linux 系统命令（见配置 allowed_commands：默认可含 **coreutils 类**（stat、grep、sort、diff、find、which…）、**系统信息**（ps、free、uptime、lscpu、lsblk…）、**压缩流读出**（zcat/bzcat/xzcat）、**JSON**（jq）、**Git / Rust**（git、cargo、rustc，dev）、**编译与 Binutils**（gcc、clang、make、cmake、ctest、mkdir、objdump…；prod 默认无编译链）等）。\n\n**command 必须是白名单中的命令名**（如 `cat`、`ls`、`cmake`），**禁止**把 `./` 开头的路径当作 command（如 `{\"command\": \"./program\"}` 是错误的）。\n\n**参数格式**：`command` 填命令名（如 `\"cat\"`），`args` 填参数数组（如 `[\"main.cpp\"]`）。**不要**把命令和参数写在一起。\n**正确示例**：`{\"command\": \"cat\", \"args\": [\"main.cpp\"]}` 或 `{\"command\": \"cmake\", \"args\": [\"--build\", \"build\"]}`\n**错误示例**：`{\"command\": \"cat main.cpp\"}` 或 `{\"command\": \"cmake --build\"}` 或 `{\"command\": \"./program\"}`\n**无参数命令**：`{\"command\": \"pwd\", \"args\": []}` 或 `{\"command\": \"ls\", \"args\": [\"-la\"]}`（pwd、whoami、lscpu 等不接受参数，不要在 args 中传参数）\n\n**禁止**：运行 `./`、`./xxx`、工作区内编译产物等**须用 run_executable**，不是 run_command。rm、mv、chmod、sudo 等未列入白名单则不可执行。\n\n【cmake 常用模式】在 build 目录外层执行配置：`cmake -S . -B build`，然后 `cmake --build build`；或在 build 目录内执行：`mkdir -p build && cd build && cmake .. && cmake --build .`（注意 cmake 参数顺序：源目录在前，构建目录在后；可用 `-B build` 指定构建目录。）。cmake 产物路径取决于构建配置（`CMAKE_RUNTIME_OUTPUT_DIRECTORY` 等），常用位置包括 `build/bin/<target>`、`build/<target>` 或 `<source_dir>/bin/<target>`，运行前请先用 `ls` 确认。",
            category: ToolCategory::Development,
            parameters: tool_params::params_run_command,
            runner: runner_run_command,
            summary: ToolSummaryKind::Dynamic(ts::summary_run_command),
        },
        ToolSpec {
            name: "run_executable",
            description: "在工作区内按**相对路径**执行可执行文件（path 如 ./main、./a.out、./build/app、target/release/foo）。**编译或构建完成后要运行产物时，必须用本工具**，不要用 run_command 拼 shell、也不要把本地程序名当成白名单命令。args 为传给该程序的参数（可选）；路径不得为绝对路径，不得含 .. 逃出工作区。\n\n【cmake 产物路径】cmake 等构建系统的输出目录由 `CMAKE_RUNTIME_OUTPUT_DIRECTORY`、`CMAKE_LIBRARY_OUTPUT_DIRECTORY` 等变量控制。默认情况下，可执行文件可能位于 `build/bin/<target>`、`build/<target>` 或 `<source_dir>/bin/<target>`。**运行前请先用 `run_command ls -la <dir>` 确认产物实际路径**。",
            category: ToolCategory::Development,
            parameters: tool_params::params_run_executable,
            runner: runner_run_executable,
            summary: ToolSummaryKind::Dynamic(ts::summary_run_executable),
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
