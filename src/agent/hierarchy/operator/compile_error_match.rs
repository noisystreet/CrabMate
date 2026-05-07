//! 编译器输出 → `CompileErrorInfo` 的启发式匹配（由 `compile` 汇总调用）。

use super::types::{CompileErrorInfo, CompileErrorType};

pub(super) fn compile_error_openmp(error_output: &str) -> Option<CompileErrorInfo> {
    if error_output.contains("not specified in enclosing 'parallel'")
        || error_output.contains("not specified in enclosing parallel")
        || error_output.contains("#pragma omp")
    {
        Some(CompileErrorInfo {
            error_type: CompileErrorType::OpenMPError,
            description: "OpenMP 并行区域变量声明错误".to_string(),
            suggested_fix: "在 setup/（或项目文档指明的模板目录）中选择不含 OpenMP 或更保守的 Makefile 模板，复制为 Make.custom 后重试"
                .to_string(),
            retryable: true,
            alternative_config: None,
        })
    } else {
        None
    }
}

pub(super) fn compile_error_missing_dependency(error_output: &str) -> Option<CompileErrorInfo> {
    if error_output.contains("cannot find -l")
        || error_output.contains("cannot find library")
        || error_output.contains("No such file or directory")
    {
        Some(CompileErrorInfo {
            error_type: CompileErrorType::MissingDependency,
            description: "缺少必要的依赖库".to_string(),
            suggested_fix: "尝试安装缺失的库或切换到不需要该库的配置".to_string(),
            retryable: true,
            alternative_config: None,
        })
    } else {
        None
    }
}

pub(super) fn compile_error_compiler_option(error_output: &str) -> Option<CompileErrorInfo> {
    if error_output.contains("unrecognized command line option")
        || error_output.contains("unknown option")
        || error_output.contains("invalid option")
    {
        Some(CompileErrorInfo {
            error_type: CompileErrorType::CompilerVersionError,
            description: "编译器版本不兼容，配置使用了不支持的选项".to_string(),
            suggested_fix: "切换到更基础的 Makefile 模板（以 read_dir + 项目文档为准）".to_string(),
            retryable: true,
            alternative_config: None,
        })
    } else {
        None
    }
}

pub(super) fn compile_error_arch_variable(error_output: &str) -> Option<CompileErrorInfo> {
    if error_output.contains("Please specify 'arch' variable")
        || error_output.contains("arch variable")
    {
        Some(CompileErrorInfo {
            error_type: CompileErrorType::ConfigError,
            description: "Makefile 需要指定 arch 参数".to_string(),
            suggested_fix: "按 README/INSTALL 使用 `make arch=<模板名>`，或将所选模板复制为 Make.custom 后再 make"
                .to_string(),
            retryable: true,
            alternative_config: None,
        })
    } else {
        None
    }
}

pub(super) fn compile_error_working_dir(error_output: &str) -> Option<CompileErrorInfo> {
    if error_output.contains("没有指明目标并且找不到 makefile")
        || error_output.contains("No targets specified and no makefile found")
        || error_output.contains("没有那个文件或目录")
        || error_output.contains("No such file or directory")
        || error_output.contains("cannot find")
        || error_output.contains("无法找到")
    {
        Some(CompileErrorInfo {
            error_type: CompileErrorType::WorkingDirectoryError,
            description: "工作目录错误或找不到构建文件".to_string(),
            suggested_fix: "使用 `make -C <源码子目录>`（目录名以 read_dir 确认为准），或把 command 改为该目录下的可执行路径"
                .to_string(),
            retryable: true,
            alternative_config: None,
        })
    } else {
        None
    }
}

pub(super) fn compile_error_link(error_output: &str) -> Option<CompileErrorInfo> {
    if error_output.contains("undefined reference")
        || error_output.contains("linker error")
        || error_output.contains("ld: ")
    {
        Some(CompileErrorInfo {
            error_type: CompileErrorType::LinkError,
            description: "链接错误".to_string(),
            suggested_fix: "检查依赖库是否正确链接".to_string(),
            retryable: false,
            alternative_config: None,
        })
    } else {
        None
    }
}

pub(super) fn compile_error_syntax(error_output: &str) -> Option<CompileErrorInfo> {
    if error_output.contains("error: expected")
        || error_output.contains("error: syntax")
        || error_output.contains("error: invalid")
    {
        Some(CompileErrorInfo {
            error_type: CompileErrorType::SyntaxError,
            description: "源代码语法错误".to_string(),
            suggested_fix: "检查源代码语法".to_string(),
            retryable: false,
            alternative_config: None,
        })
    } else {
        None
    }
}

pub(super) fn analyze_compile_error(error_output: &str) -> Option<CompileErrorInfo> {
    compile_error_openmp(error_output)
        .or_else(|| compile_error_missing_dependency(error_output))
        .or_else(|| compile_error_compiler_option(error_output))
        .or_else(|| compile_error_arch_variable(error_output))
        .or_else(|| compile_error_working_dir(error_output))
        .or_else(|| compile_error_link(error_output))
        .or_else(|| compile_error_syntax(error_output))
}
