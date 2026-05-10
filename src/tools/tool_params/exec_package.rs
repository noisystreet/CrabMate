//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::PackageQueryArgs;

pub(in crate::tools) fn params_terminal_session() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["exec", "send_signal", "resize", "list", "close"],
                "description": "**exec**：启动伪终端（无 `session_id` 时须提供 `command`+可选 `args`，规则同 **`run_command`**）或向既有会话写 **`input`**。**send_signal** / **resize** / **close** / **list**：会话管理。"
            },
            "session_id": { "type": "string", "description": "会话 ID（`pty*`；由首轮 exec 返回或 **list** 列出）。" },
            "command": { "type": "string", "description": "与 **`run_command`** 相同：纯命令名（新建 exec 必填）。" },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "命令参数（可选）。"
            },
            "input": { "type": "string", "description": "写入 PTY 的字节流（可选）。" },
            "signal": { "type": "integer", "description": "**send_signal**：Unix 信号编号（整数）。" },
            "cols": { "type": "integer", "minimum": 1, "description": "终端宽度列（可选，默认 80）。" },
            "rows": { "type": "integer", "minimum": 1, "description": "终端高度行（可选，默认 24）。" }
        },
        "required": ["action"]
    })
}

pub(in crate::tools) fn params_run_command() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "**⚠️ 重要：command 必须是纯命令名，不能包含任何参数！**\n\n- 白名单系统命令（如 ls、find、grep、stat、git、gh、cargo、gcc、cmake、ctest、mkdir、make、file、jq 等，完整列表见 config/tools.toml）\n- 工作区相对路径（如 ./build/app、scripts/test.sh）\n\n**command 字段只填命令名或路径，参数必须放在 args 数组中。禁止在 command 中包含任何选项或参数！**\n\n**✅ 正确格式**：`{\"command\": \"cmake\", \"args\": [\"--build\", \"build\"]}`\n**❌ 错误格式**：`{\"command\": \"cmake --build\", \"args\": [\"build\"]}` 或 `{\"command\": \"cat main.cpp\"}`\n\n常见错误：\n- `cmake --build` → 应拆分为 `command: \"cmake\", args: [\"--build\", \"build\"]`\n- `cat main.cpp` → 应拆分为 `command: \"cat\", args: [\"main.cpp\"]`\n- `ls -la` → 应拆分为 `command: \"ls\", args: [\"-la\", \"src/\"]`\n- `which cmake` 写在 `command` 一个字段里 → 应 `command: \"which\", args: [\"cmake\"]`（`which` 为白名单时）"
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
    tool_parameters_schema_value::<PackageQueryArgs>()
}
