//! 统一工具执行结果：用于工作流等编排场景的结构化状态判断。

#[derive(Debug, Clone)]
pub struct ToolResult {
    /// 工具调用是否成功（由退出码或错误语义推断）
    pub ok: bool,
    /// 若输出可解析出退出码，则填充该字段
    pub exit_code: Option<i32>,
    /// 原始输出（兼容现有前端/模型消费逻辑）
    pub message: String,
    /// 若可抽取，标准输出文本
    pub stdout: String,
    /// 若可抽取，标准错误文本
    pub stderr: String,
    /// 机器可读错误码（失败时填充）
    pub error_code: Option<String>,
}

impl ToolResult {
    /// 将既有“字符串工具输出”转换为结构化结果。
    pub fn from_legacy_output(tool_name: &str, output: String) -> Self {
        let first = output.lines().next().unwrap_or("").trim();
        let exit_code = parse_exit_code(first);
        let (stdout, stderr) = extract_streams(&output);

        let ok = if let Some(code) = exit_code {
            code == 0
        } else {
            !looks_like_failure(first)
        };
        let error_code = if ok {
            None
        } else {
            Some(classify_error_code(first, tool_name))
        };

        Self {
            ok,
            exit_code,
            message: output,
            stdout,
            stderr,
            error_code,
        }
    }
}

fn parse_exit_code(first_line: &str) -> Option<i32> {
    if let Some(rest) = first_line.strip_prefix("退出码：") {
        return rest.trim().parse::<i32>().ok();
    }
    let idx = first_line.find("(exit=")?;
    let rest = &first_line[idx + "(exit=".len()..];
    let end = rest.find(')')?;
    rest[..end].trim().parse::<i32>().ok()
}

fn looks_like_failure(first_line: &str) -> bool {
    if first_line.is_empty() {
        return false;
    }
    first_line.starts_with("错误")
        || first_line.starts_with("未知工具")
        || first_line.starts_with("参数解析错误")
        || first_line.starts_with("执行失败")
        || first_line.contains("失败")
        || first_line.contains("超时")
}

fn classify_error_code(first_line: &str, tool_name: &str) -> String {
    if first_line.contains("参数解析错误") {
        return "invalid_args".to_string();
    }
    if first_line.contains("不允许的命令") {
        return "command_not_allowed".to_string();
    }
    if first_line.contains("未设置工作区") {
        return "workspace_not_set".to_string();
    }
    if first_line.contains("超时") {
        return "timeout".to_string();
    }
    if first_line.starts_with("未知工具") {
        return "unknown_tool".to_string();
    }
    format!("{}_failed", tool_name)
}

fn extract_streams(output: &str) -> (String, String) {
    let stdout_marker = "标准输出：\n";
    let stderr_marker = "标准错误：\n";

    let stdout = if let Some(pos) = output.find(stdout_marker) {
        let start = pos + stdout_marker.len();
        let end = output[start..]
            .find(stderr_marker)
            .map(|i| start + i)
            .unwrap_or(output.len());
        output[start..end].trim().to_string()
    } else {
        String::new()
    };
    let stderr = if let Some(pos) = output.find(stderr_marker) {
        let start = pos + stderr_marker.len();
        output[start..].trim().to_string()
    } else {
        String::new()
    };
    (stdout, stderr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exit_code_from_chinese_prefix() {
        let r = ToolResult::from_legacy_output(
            "run_command",
            "退出码：0\n标准输出：\nhello\n".to_string(),
        );
        assert!(r.ok);
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.stdout, "hello");
    }

    #[test]
    fn parse_exit_code_from_exit_pattern() {
        let r = ToolResult::from_legacy_output("cargo_test", "cargo test (exit=1):\nfailed".to_string());
        assert!(!r.ok);
        assert_eq!(r.exit_code, Some(1));
        assert_eq!(r.error_code.as_deref(), Some("cargo_test_failed"));
    }

    #[test]
    fn classify_workspace_error_without_exit_code() {
        let r = ToolResult::from_legacy_output("run_command", "错误：未设置工作区".to_string());
        assert!(!r.ok);
        assert_eq!(r.exit_code, None);
        assert_eq!(r.error_code.as_deref(), Some("workspace_not_set"));
    }
}
