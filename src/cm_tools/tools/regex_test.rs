//! 正则表达式测试工具（纯内存）

use crate::cm_tools::tool_result::ToolError;
use regex::Regex;

/// 显式失败版本（成功正文与迁移前的 String 面逐字节一致）。
///
/// 本族无内部 String 消费者，故不保留 `run` 门面；测试用本地 `run_str` 还原旧形态。
#[allow(clippy::result_large_err)]
pub fn run_try(args_json: &str) -> Result<String, ToolError> {
    let args: super::tool_param_types::RegexTestArgs =
        super::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    let pattern = args.pattern.trim();
    if pattern.is_empty() {
        return Err(ToolError::invalid_args("错误：缺少 pattern 参数".to_string()));
    }
    let re = Regex::new(pattern).map_err(|e| {
        ToolError::external_code("regex_test_invalid_regex", format!("正则编译失败：{}", e))
    })?;
    let test_strings: Vec<&str> = args.test_strings.iter().map(String::as_str).collect();
    if test_strings.is_empty() {
        return Err(ToolError::invalid_args(
            "错误：缺少 test_strings 数组参数".to_string(),
        ));
    }
    if test_strings.len() > 100 {
        return Err(ToolError::invalid_args(
            "错误：test_strings 上限 100 条".to_string(),
        ));
    }

    let mut results = Vec::new();
    for s in &test_strings {
        if let Some(m) = re.captures(s) {
            let groups: Vec<String> = m
                .iter()
                .enumerate()
                .filter_map(|(i, g)| g.map(|g| format!("  group[{}]: {:?}", i, g.as_str())))
                .collect();
            results.push(format!("✓ {:?}\n{}", s, groups.join("\n")));
        } else {
            results.push(format!("✗ {:?}", s));
        }
    }
    let matched = results.iter().filter(|r| r.starts_with('✓')).count();
    Ok(format!(
        "pattern: {}\n匹配: {}/{}\n\n{}",
        pattern,
        matched,
        test_strings.len(),
        results.join("\n\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 迁移前 String 面的等价物（`ToolError` → `message`），供断言旧文案用。
    fn run_str(args_json: &str) -> String {
        run_try(args_json).unwrap_or_else(|e| e.message)
    }

    #[test]
    fn basic_match() {
        let out = run_str(r#"{"pattern":"\\d+","test_strings":["abc123","hello"]}"#);
        assert!(out.contains("匹配: 1/2"));
        assert!(out.contains('✓'));
        assert!(out.contains('✗'));
    }

    #[test]
    fn capture_groups() {
        let out = run_str(r#"{"pattern":"(\\w+)@(\\w+)","test_strings":["user@host"]}"#);
        assert!(out.contains("group[1]"));
        assert!(out.contains("group[2]"));
    }

    #[test]
    fn invalid_regex() {
        let out = run_str(r#"{"pattern":"[invalid","test_strings":["x"]}"#);
        assert!(out.contains("正则编译失败"));
    }

    #[test]
    fn empty_pattern() {
        let out = run_str(r#"{"pattern":"","test_strings":["x"]}"#);
        assert!(out.contains("缺少 pattern"));
    }
}
