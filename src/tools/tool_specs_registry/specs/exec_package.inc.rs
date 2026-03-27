[
ToolSpec {
            name: "run_command",
            description: "在服务器上执行**白名单内**的 Linux 系统命令（见配置 allowed_commands：默认可含 **coreutils 类**（stat、grep、sort、diff、find、which…）、**系统信息**（ps、free、uptime、lscpu、lsblk…）、**压缩流读出**（zcat/bzcat/xzcat）、**JSON**（jq）、**Git / Rust**（git、cargo、rustc，dev）、**编译与 Binutils**（gcc、clang、make、cmake、objdump…；prod 默认无编译链）等）。**find** 可递归列文件；`-exec`/`-delete` 等可改系统或执行任意程序，请在**信任工作区**使用。**不要**用本工具运行工作区内已生成的可执行文件（./main、./a.out…），须用 **run_executable**。参数 args 为字符串数组；禁止含 \"..\" 或以 \"/\" 开头的实参。rm、mv、chmod、sudo 等未列入白名单则不可执行。",
            category: ToolCategory::Development,
            parameters: tool_params::params_run_command,
            runner: runner_run_command,
            summary: ToolSummaryKind::Dynamic(ts::summary_run_command),
        },
        ToolSpec {
            name: "run_executable",
            description: "在工作区内按**相对路径**执行可执行文件（path 如 ./main、./a.out、./build/app、target/release/foo）。**编译或构建完成后要运行产物时，必须用本工具**，不要用 run_command 拼 shell、也不要把本地程序名当成白名单命令。args 为传给该程序的参数（可选）；路径不得为绝对路径，不得含 .. 逃出工作区。",
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
