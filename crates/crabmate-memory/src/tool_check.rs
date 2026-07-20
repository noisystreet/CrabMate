//! 精简工具结果检查（从 `crabmate_tools::tool_result` 提取）。
//!
//! 仅检查工具是否成功，不涉及完整的 `NormalizedToolEnvelope` 解析或结构化输出。

/// 从工具消息正文判断工具是否成功。
///
/// 1. 若为 JSON（标准 / CrabMate 信封），取顶层或 `crabmate_tool.ok` 字段；
/// 2. 若为纯文本 `run_command` 风格，检查 `退出码：0`。
pub(crate) fn tool_output_is_ok(content: &str) -> bool {
    let t = content.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
        // CrabMate 信封格式
        if let Some(env) = v.get("crabmate_tool") {
            return env.get("ok").and_then(|x| x.as_bool()) == Some(true);
        }
        // 普通 JSON 格式
        return v.get("ok").and_then(|x| x.as_bool()) != Some(false);
    }
    // 纯文本格式
    for line in t.lines() {
        let line = line.trim();
        if line.starts_with("退出码：") {
            return line.trim_start_matches("退出码：").trim() == "0";
        }
        if let Some(pos) = line.find("(exit=")
            && let Some(end) = line[pos..].find(')')
        {
            let code_str = &line[pos + 6..pos + end];
            return code_str.trim() == "0";
        }
    }
    false
}
