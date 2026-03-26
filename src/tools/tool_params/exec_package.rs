//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_run_command() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "命令名（小写），须为配置中 allowed_commands 白名单之一（如 ls、gcc、cmake、make、file 等）。**不要**用本工具运行工作区内的可执行文件（例如 ./main、./a.out、./build/app）；此类请改用 **run_executable**，参数 path 填相对工作目录的路径。"
            },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "传给白名单命令的参数（可选）。**不要**用 args 拼出「执行当前目录下程序」——应使用 run_executable。"
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
                "description": "相对工作目录的可执行文件路径（如 ./main、./a.out、./build/app）。编译或构建得到的程序应**优先用本工具运行**，不要用 run_command。"
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
