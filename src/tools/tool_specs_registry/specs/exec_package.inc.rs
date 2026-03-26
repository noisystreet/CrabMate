[
ToolSpec {
            name: "run_command",
            description: "在服务器上执行**白名单内**的 Linux 系统命令（见配置 allowed_commands：如 ls、gcc、cmake、make、file、**GNU Binutils 只读分析**（objdump、nm、readelf、strings、size；dev 另含 ar）等）。用于列目录、读文件、**编译/链接**（gcc/clang/make/ninja/cmake）、Autotools、c++filt、**ELF/目标文件反汇编与符号查看**等。**不要**用本工具去「运行当前工作区里已生成的可执行文件」（./main、./a.out、./build/…）；那种情况必须用 **run_executable**。参数 args 为字符串数组；禁止含 \"..\" 或以 \"/\" 开头的实参。不要执行 rm、mv、chmod 等未在白名单中的命令。",
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
