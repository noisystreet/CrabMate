//! 环境变量批量检查工具（只读、脱敏）

use crate::cm_tools::tool_result::ToolError;

fn format_one_env_var_line(
    raw_name: &str,
    sanitized: &str,
    val: Result<String, std::env::VarError>,
    show_length: bool,
    show_prefix: usize,
) -> String {
    if sanitized.is_empty() {
        return format!("  {}: (无效变量名)", raw_name);
    }
    match val {
        Ok(val) => {
            let mut info = if val.is_empty() {
                "已设置（空值）".to_string()
            } else {
                "已设置".to_string()
            };
            if !val.is_empty() {
                if show_length {
                    info.push_str(&format!("，长度={}", val.len()));
                }
                if show_prefix > 0 {
                    let prefix: String = val.chars().take(show_prefix).collect();
                    info.push_str(&format!("，前缀={:?}…", prefix));
                }
            }
            format!("  {}: {}", sanitized, info)
        }
        Err(_) => format!("  {}: 未设置", sanitized),
    }
}

/// 显式失败版本（成功正文与迁移前的 String 面逐字节一致）。
///
/// 本族无内部 String 消费者，故不保留 `run` 门面；测试用本地 `run_str` 还原旧形态。
#[allow(clippy::result_large_err)]
pub fn run_try(args_json: &str) -> Result<String, ToolError> {
    let v = super::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    if v.get("names").is_none() {
        return Err(ToolError::invalid_args(
            "错误：缺少 names 数组参数".to_string(),
        ));
    }
    let args: super::tool_param_types::EnvVarCheckArgs = serde_json::from_value(v)
        .map_err(|e| ToolError::invalid_args(format!("参数 JSON 无效: {e}")))?;
    let names: Vec<&str> = args.names.iter().map(String::as_str).collect();
    if names.is_empty() {
        return Err(ToolError::invalid_args(
            "错误：names 不能为空".to_string(),
        ));
    }
    if names.len() > 50 {
        return Err(ToolError::invalid_args(
            "错误：names 上限 50 个".to_string(),
        ));
    }

    let show_length = args.show_length.unwrap_or(false);
    let show_prefix = args.show_prefix_chars.unwrap_or(0).min(8) as usize;

    let mut lines: Vec<String> = Vec::new();
    let mut set_count = 0usize;
    for name in &names {
        let sanitized = name
            .trim()
            .replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "");
        let val = std::env::var(&sanitized);
        if val.is_ok() {
            set_count += 1;
        }
        lines.push(format_one_env_var_line(
            name,
            &sanitized,
            val,
            show_length,
            show_prefix,
        ));
    }
    Ok(format!(
        "环境变量检查（{}/{}已设置）：\n{}",
        set_count,
        names.len(),
        lines.join("\n")
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
    fn check_path_is_set() {
        let out = run_str(r#"{"names":["PATH"]}"#);
        assert!(out.contains("已设置"), "PATH should be set, got: {}", out);
        assert!(out.contains("1/1"));
    }

    #[test]
    fn check_nonexistent_var() {
        let out = run_str(r#"{"names":["CM_TEST_NONEXISTENT_VAR_XYZ"]}"#);
        assert!(out.contains("未设置"), "out={}", out);
        assert!(out.contains("0/1"));
    }

    #[test]
    fn show_length() {
        let out = run_str(r#"{"names":["PATH"],"show_length":true}"#);
        assert!(out.contains("长度="), "out={}", out);
    }

    #[test]
    fn empty_names() {
        let out = run_str(r#"{"names":[]}"#);
        assert!(out.contains("不能为空"));
    }

    #[test]
    fn missing_names() {
        let out = run_str(r#"{}"#);
        assert!(out.contains("缺少 names"));
    }
}
