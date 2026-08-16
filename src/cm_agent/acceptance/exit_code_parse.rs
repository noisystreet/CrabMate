//! 从合并工具/子目标输出文本中解析退出码（分阶段 legacy 与分层子目标摘要共用）。

/// 匹配「退出码：0」「(exit=1)」「exit code: 0」等常见形态。
pub fn parse_exit_code_from_combined_output(output: &str) -> Option<i32> {
    const PATTERNS: &[&str] = &["退出码：", "exit=", "exit code: ", "exit code:", "(exit="];

    for pattern in PATTERNS {
        if let Some(pos) = output.find(pattern) {
            let start = pos + pattern.len();
            let rest = &output[start..];
            let num_str: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '-')
                .collect();
            if let Ok(code) = num_str.parse::<i32>() {
                return Some(code);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_exit_code_patterns() {
        assert_eq!(parse_exit_code_from_combined_output("退出码：0"), Some(0));
        assert_eq!(parse_exit_code_from_combined_output("(exit=1)"), Some(1));
        assert_eq!(
            parse_exit_code_from_combined_output("exit code: 127"),
            Some(127)
        );
        assert!(parse_exit_code_from_combined_output("some output").is_none());
    }
}
