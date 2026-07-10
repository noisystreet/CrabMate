//! 环境变量批量检查工具（只读、脱敏）

pub fn run(args_json: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {e}"),
    };
    if v.get("names").is_none() {
        return "错误：缺少 names 数组参数".to_string();
    }
    let args: super::tool_param_types::EnvVarCheckArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 无效: {e}"),
    };
    let names: Vec<&str> = args.names.iter().map(String::as_str).collect();
    if names.is_empty() {
        return "错误：names 不能为空".to_string();
    }
    if names.len() > 50 {
        return "错误：names 上限 50 个".to_string();
    }

    let show_length = args.show_length.unwrap_or(false);
    let show_prefix = args.show_prefix_chars.unwrap_or(0).min(8) as usize;

    let mut lines: Vec<String> = Vec::new();
    let mut set_count = 0usize;
    for name in &names {
        let sanitized = name
            .trim()
            .replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "");
        if sanitized.is_empty() {
            lines.push(format!("  {}: (无效变量名)", name));
            continue;
        }
        match std::env::var(&sanitized) {
            Ok(val) => {
                set_count += 1;
                let mut info = "已设置".to_string();
                if val.is_empty() {
                    info = "已设置（空值）".to_string();
                } else {
                    if show_length {
                        info.push_str(&format!("，长度={}", val.len()));
                    }
                    if show_prefix > 0 && !val.is_empty() {
                        let prefix: String = val.chars().take(show_prefix).collect();
                        info.push_str(&format!("，前缀={:?}…", prefix));
                    }
                }
                lines.push(format!("  {}: {}", sanitized, info));
            }
            Err(_) => {
                lines.push(format!("  {}: 未设置", sanitized));
            }
        }
    }
    format!(
        "环境变量检查（{}/{}已设置）：\n{}",
        set_count,
        names.len(),
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_path_is_set() {
        let out = run(r#"{"names":["PATH"]}"#);
        assert!(out.contains("已设置"), "PATH should be set, got: {}", out);
        assert!(out.contains("1/1"));
    }

    #[test]
    fn check_nonexistent_var() {
        let out = run(r#"{"names":["CM_TEST_NONEXISTENT_VAR_XYZ"]}"#);
        assert!(out.contains("未设置"), "out={}", out);
        assert!(out.contains("0/1"));
    }

    #[test]
    fn show_length() {
        let out = run(r#"{"names":["PATH"],"show_length":true}"#);
        assert!(out.contains("长度="), "out={}", out);
    }

    #[test]
    fn empty_names() {
        let out = run(r#"{"names":[]}"#);
        assert!(out.contains("不能为空"));
    }

    #[test]
    fn missing_names() {
        let out = run(r#"{}"#);
        assert!(out.contains("缺少 names"));
    }
}
