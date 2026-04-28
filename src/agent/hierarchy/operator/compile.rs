//! 编译错误解析与恢复提示文案。

use super::types::{CompileErrorInfo, CompileErrorType};

/// 分析编译错误并返回错误信息
pub(crate) fn analyze_compile_error(error_output: &str) -> Option<CompileErrorInfo> {
    let _error_lower = error_output.to_lowercase();

    if error_output.contains("not specified in enclosing 'parallel'")
        || error_output.contains("not specified in enclosing parallel")
        || error_output.contains("#pragma omp")
    {
        return Some(CompileErrorInfo {
            error_type: CompileErrorType::OpenMPError,
            description: "OpenMP 并行区域变量声明错误".to_string(),
            suggested_fix: "在 setup/（或项目文档指明的模板目录）中选择不含 OpenMP 或更保守的 Makefile 模板，复制为 Make.custom 后重试"
                .to_string(),
            retryable: true,
            alternative_config: None,
        });
    }

    if error_output.contains("cannot find -l")
        || error_output.contains("cannot find library")
        || error_output.contains("No such file or directory")
    {
        return Some(CompileErrorInfo {
            error_type: CompileErrorType::MissingDependency,
            description: "缺少必要的依赖库".to_string(),
            suggested_fix: "尝试安装缺失的库或切换到不需要该库的配置".to_string(),
            retryable: true,
            alternative_config: None,
        });
    }

    if error_output.contains("unrecognized command line option")
        || error_output.contains("unknown option")
        || error_output.contains("invalid option")
    {
        return Some(CompileErrorInfo {
            error_type: CompileErrorType::CompilerVersionError,
            description: "编译器版本不兼容，配置使用了不支持的选项".to_string(),
            suggested_fix: "切换到更基础的 Makefile 模板（以 read_dir + 项目文档为准）".to_string(),
            retryable: true,
            alternative_config: None,
        });
    }

    if error_output.contains("Please specify 'arch' variable")
        || error_output.contains("arch variable")
    {
        return Some(CompileErrorInfo {
            error_type: CompileErrorType::ConfigError,
            description: "Makefile 需要指定 arch 参数".to_string(),
            suggested_fix: "按 README/INSTALL 使用 `make arch=<模板名>`，或将所选模板复制为 Make.custom 后再 make"
                .to_string(),
            retryable: true,
            alternative_config: None,
        });
    }

    if error_output.contains("没有指明目标并且找不到 makefile")
        || error_output.contains("No targets specified and no makefile found")
        || error_output.contains("没有那个文件或目录")
        || error_output.contains("No such file or directory")
        || error_output.contains("cannot find")
        || error_output.contains("无法找到")
    {
        return Some(CompileErrorInfo {
            error_type: CompileErrorType::WorkingDirectoryError,
            description: "工作目录错误或找不到构建文件".to_string(),
            suggested_fix: "使用 `make -C <源码子目录>`（目录名以 read_dir 确认为准），或把 command 改为该目录下的可执行路径"
                .to_string(),
            retryable: true,
            alternative_config: None,
        });
    }

    if error_output.contains("undefined reference")
        || error_output.contains("linker error")
        || error_output.contains("ld: ")
    {
        return Some(CompileErrorInfo {
            error_type: CompileErrorType::LinkError,
            description: "链接错误".to_string(),
            suggested_fix: "检查依赖库是否正确链接".to_string(),
            retryable: false,
            alternative_config: None,
        });
    }

    if error_output.contains("error: expected")
        || error_output.contains("error: syntax")
        || error_output.contains("error: invalid")
    {
        return Some(CompileErrorInfo {
            error_type: CompileErrorType::SyntaxError,
            description: "源代码语法错误".to_string(),
            suggested_fix: "检查源代码语法".to_string(),
            retryable: false,
            alternative_config: None,
        });
    }

    None
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
