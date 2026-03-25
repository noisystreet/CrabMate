//! 正则表达式测试工具（纯内存）

use regex::Regex;

pub fn run(args_json: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let pattern = match v.get("pattern").and_then(|x| x.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "错误：缺少 pattern 参数".to_string(),
    };
    let re = match Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => return format!("正则编译失败：{}", e),
    };
    let test_strings = match v.get("test_strings").and_then(|x| x.as_array()) {
        Some(arr) => arr.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>(),
        None => return "错误：缺少 test_strings 数组参数".to_string(),
    };
    if test_strings.is_empty() {
        return "错误：test_strings 不能为空".to_string();
    }
    if test_strings.len() > 100 {
        return "错误：test_strings 上限 100 条".to_string();
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
    format!(
        "pattern: {}\n匹配: {}/{}\n\n{}",
        pattern,
        matched,
        test_strings.len(),
        results.join("\n\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_match() {
        let out = run(r#"{"pattern":"\\d+","test_strings":["abc123","hello"]}"#);
        assert!(out.contains("匹配: 1/2"));
        assert!(out.contains('✓'));
        assert!(out.contains('✗'));
    }

    #[test]
    fn capture_groups() {
        let out = run(r#"{"pattern":"(\\w+)@(\\w+)","test_strings":["user@host"]}"#);
        assert!(out.contains("group[1]"));
        assert!(out.contains("group[2]"));
    }

    #[test]
    fn invalid_regex() {
        let out = run(r#"{"pattern":"[invalid","test_strings":["x"]}"#);
        assert!(out.contains("正则编译失败"));
    }

    #[test]
    fn empty_pattern() {
        let out = run(r#"{"pattern":"","test_strings":["x"]}"#);
        assert!(out.contains("缺少 pattern"));
    }
}
