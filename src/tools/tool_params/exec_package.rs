//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_run_command() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "**命令名或相对路径**：\n- 白名单系统命令（如 ls、find、grep、stat、git、gh、cargo、gcc、cmake、ctest、mkdir、make、file、jq 等，完整列表见 config/tools.toml）\n- 工作区相对路径（如 ./build/app、scripts/test.sh）\n\n**仅填命令名或路径本身，不要把参数也填进来**。\n\n**正确示例**：`{\"command\": \"cat\", \"args\": [\"main.cpp\"]}`、`{\"command\": \"./build/app\", \"args\": [\"--help\"]}`、`{\"command\": \"bash\", \"args\": [\"script.sh\"]}`、`{\"command\": \"cmake\", \"args\": [\"-S\", \".\", \"-B\", \"build\"]}`\n**错误示例**：`{\"command\": \"cat main.cpp\"}`、`{\"command\": \"cmake --build\"}`"
            },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "**传给命令的参数数组**（可选），如 [\"main.cpp\"] 或 [\"-la\", \"src/\"]。**不要**把命令和参数写在一起；command 只填命令名或路径，参数全放 args。**禁止**在 args 中传入绝对路径（以 / 开头）或含 .. 的参数。"
            }
        },
        "required": ["command"]
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
