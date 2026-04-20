[
ToolSpec {
            name: "run_command",
            description: "在服务器上执行 Linux 命令。分两种模式：\n\n**1. 白名单系统命令**：command 必须是配置中 allowed_commands 白名单之一（默认可含 **coreutils 类**：stat、grep、sort、diff、find、which…；**系统信息**：ps、free、uptime、lscpu、lsblk…；**压缩流读出**：zcat/bzcat/xzcat；**JSON**：jq；**Git / Rust**：git、gh、cargo、rustc；**编译与 Binutils**：gcc、clang、make、cmake、ctest、mkdir、objdump…；prod 默认无编译链等）。\n\n**2. 工作区脚本/可执行文件**：若 command 形如 `./xxx`、`path/to/script` 等相对路径，且解析到工作区内已存在的可执行文件或常见脚本（sh/bash/py/pl/rb/js/ts 等），会**自动审批通过**（跳过白名单人工确认）。\n\n**参数格式**：`command` 填命令名或相对路径，`args` 填参数数组（每个参数独立，不包含命令名）。**不要**把命令和参数混在一起。\n**正确示例**：`{\"command\": \"cat\", \"args\": [\"main.cpp\"]}`、`{\"command\": \"which\", \"args\": [\"gcc\", \"g++\"]}`、`{\"command\": \"./build/app\", \"args\": [\"--help\"]}`\n**常见错误**：`{\"command\": \"cat main.cpp\"}`（命令和参数混在一起）、`{\"command\": \"which\", \"args\": [\"which gcc\"]}`（args 包含了命令名，应只写 `[\"gcc\"]`）、`{\"command\": \"cmake --build\"}`（应该分开为 `\"cmake\"` 和 `[\"--build\"]`）\n\n**禁止**：rm、mv、chmod、sudo 等未列入白名单则不可执行。\n\n【cmake 常用模式】\n1. **推荐：使用 -S 和 -B 参数**（在源目录外配置）：`cmake -S . -B build` 然后 `cmake --build build`\n2. **传统方式**：`mkdir -p build && cmake ..`（在 build 目录内执行 .. 配置源目录）\n3. cmake 产物路径取决于构建配置，常用位置包括 `build/bin/<target>`、`build/<target>`，运行前请先用 `ls build` 确认",
            category: ToolCategory::Development,
            parameters: tool_params::params_run_command,
            runner: runner_run_command,
            summary: ToolSummaryKind::Dynamic(ts::summary_run_command),
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