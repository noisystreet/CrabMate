//! 编译错误解析与恢复提示文案。

use super::compile_error_match;
use super::types::CompileErrorInfo;

/// 分析编译错误并返回错误信息
pub(crate) fn analyze_compile_error(error_output: &str) -> Option<CompileErrorInfo> {
    compile_error_match::analyze_compile_error(error_output)
}

/// 构建编译错误恢复提示
pub(crate) fn build_compile_error_recovery_hint(error_info: &CompileErrorInfo) -> String {
    format!(
        r#"检测到编译错误：{}

错误类型：{:?}
建议修复方案：{}
{}

请在下一步工具调用中应用上述修复方案。"#,
        error_info.description,
        error_info.error_type,
        error_info.suggested_fix,
        if let Some(ref config) = error_info.alternative_config {
            format!("\n建议尝试的配置模板：{}", config)
        } else {
            String::new()
        }
    )
}

#[derive(Debug, Clone)]
pub(crate) struct CompileErrorMetrics {
    pub error_count: usize,
    pub first_error_signature: String,
}

pub(crate) fn parse_compile_error_metrics(output: &str) -> Option<CompileErrorMetrics> {
    let mut error_lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| {
            l.contains(" error:")
                || l.starts_with("error:")
                || l.starts_with("error[")
                || l.contains(": error:")
        })
        .collect();
    if error_lines.is_empty() {
        return None;
    }
    let first = error_lines.remove(0).to_string();
    Some(CompileErrorMetrics {
        error_count: error_lines.len() + 1,
        first_error_signature: first,
    })
}

/// 判断工具调用是否是编译相关命令
pub(crate) fn is_compile_command(tool_name: &str, args: &str) -> bool {
    if tool_name != "run_command" {
        return false;
    }

    let args_lower = args.to_lowercase();
    let compile_keywords = [
        "make",
        "cmake",
        "gcc",
        "g++",
        "clang",
        "clang++",
        "configure",
        "build",
        "compile",
        "arch=",
    ];

    compile_keywords.iter().any(|kw| args_lower.contains(kw))
}

pub(crate) fn is_convergence_compile_fix_goal(goal: &super::super::task::SubGoal) -> bool {
    let d = goal.description.to_lowercase();
    (d.contains("修复") || d.contains("fix") || d.contains("排错") || d.contains("debug"))
        && (d.contains("编译")
            || d.contains("构建")
            || d.contains("build")
            || d.contains("cargo check")
            || d.contains("cargo build"))
}
