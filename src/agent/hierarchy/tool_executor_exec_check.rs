//! [`crate::agent::hierarchy::tool_executor::ToolExecutor::run_tool`] 使用的成功判定启发式（降低 `tool_executor` 圈复杂度）。

pub(super) fn check_execution_success(tool_name: &str, output: &str) -> bool {
    let has_explicit_error = output_has_explicit_error_markers(output);
    if infer_build_like_command(tool_name, output) {
        build_like_command_succeeded(output, has_explicit_error)
    } else {
        !has_explicit_error
    }
}

fn output_has_explicit_error_markers(output: &str) -> bool {
    output.contains("错误：")
        || output.contains("error:")
        || output.contains("Error:")
        || output.contains("致命错误")
        || output.contains("fatal error")
}

fn infer_build_like_command(tool_name: &str, output: &str) -> bool {
    matches!(tool_name, "run_command" | "cmake" | "make")
        || output.contains("make:")
        || output.contains("g++")
        || output.contains("gcc")
        || output.contains("cmake")
}

fn build_like_command_succeeded(output: &str, has_explicit_error: bool) -> bool {
    if has_explicit_error {
        return false;
    }
    if output.lines().any(line_looks_like_compiler_error) {
        return false;
    }
    if output.contains("make: ***") && output.contains("停止") {
        return false;
    }
    true
}

fn line_looks_like_compiler_error(line: &str) -> bool {
    let line_lower = line.to_lowercase();
    line_lower.contains("error:")
        && !line_lower.contains("warning:")
        && !line_lower.contains("note:")
        && !line_lower.contains("0 errors")
        && !line_lower.contains("no errors")
}
