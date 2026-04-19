//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_run_command() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "**命令名（小写）**，须为配置中 allowed_commands 白名单之一（如 ls、find、grep、stat、git、gh、cargo、gcc、cmake、ctest、mkdir、make、file、jq 等，完整列表见 config/tools.toml）。**仅填命令名本身（如 \"cat\" 或 \"cmake\"），不要把参数也填进来**。**禁止把 `./program` 当作 command（如 `{\"command\": \"./program\"}` 是错误的），此类请改用 run_executable**。\n\n**正确示例**：`{\"command\": \"cat\", \"args\": [\"main.cpp\"]}` 或 `{\"command\": \"cmake\", \"args\": [\"--build\", \"build\"]}`\n**错误示例**：`{\"command\": \"cat main.cpp\"}` 或 `{\"command\": \"cmake --build\"}` 或 `{\"command\": \"./program\"}`"
            },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "**传给命令的参数数组**（可选），如 [\"main.cpp\"] 或 [\"-la\", \"src/\"]。**不要**把命令和参数写在一起；command 只填命令名，参数全放 args。"
            }
        },
        "required": ["command"]
    })
}

pub(in crate::tools) fn params_run_executable() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作目录的可执行文件路径（如 ./main、./a.out、./build/app）。编译或构建得到的程序应**优先用本工具运行**，不要用 run_command。\n\n**注意**：cmake 等构建系统的产物路径取决于构建目录配置。若用 `cmake -S . -B build` 在 build 目录构建，则产物在 `build/<target>` 或 `build/bin/<target>` 下；若在 build 目录内用 `cmake ..`，则产物在当前目录。先用 `run_command ls -la <dir>` 确认产物实际路径后再调用本工具。"
            },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "传给程序的参数（可选），如 [\"--help\"], [\"arg1\", \"arg2\"]"
            }
        },
        "required": ["path"]
    })
}

pub(in crate::tools) fn params_package_query() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": {
                "type": "string",
                "description": "要查询的包名（如 bash、curl、openssl、libc6:amd64）。仅支持字母、数字及 . + - _ : @。"
            },
            "manager": {
                "type": "string",
                "description": "包管理器：auto（默认，优先 apt 后 rpm）、apt、rpm。",
                "enum": ["auto", "apt", "rpm"]
            }
        },
        "required": ["package"],
        "additionalProperties": false
    })
}
