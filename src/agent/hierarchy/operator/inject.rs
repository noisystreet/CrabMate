//! 产物占位符注入与工作目录探测。

use std::path::PathBuf;

use crate::types::ToolCall;

use super::super::artifact_resolver::ArtifactResolver;
use super::super::tool_executor::ToolExecutionResult;

fn replace_ref_placeholders(result: &mut String, resolver: &ArtifactResolver<'_>) -> bool {
    let mut modified = false;
    const REF: &str = "{ref:";
    let mut s_idx = 0;
    while let Some(i) = result[s_idx..].find(REF) {
        let abs = s_idx + i;
        if let Some(e) = result[abs..].find('}') {
            let end = abs + e;
            let inner = &result[abs + REF.len()..=end - 1];
            if let Some((from_goal, art_id)) = inner.split_once(':')
                && let Some(p) = resolver.resolve_ref(from_goal.trim(), art_id.trim())
            {
                let path_str = p.to_string_lossy();
                result.replace_range(abs..=end, &path_str);
                modified = true;
                s_idx = abs + path_str.len();
                continue;
            }
        }
        s_idx = abs + 1;
        if s_idx >= result.len() {
            break;
        }
    }
    modified
}

fn replace_artifact_placeholders(result: &mut String, resolver: &ArtifactResolver<'_>) -> bool {
    let mut modified = false;
    let pattern = "{artifact:";
    let mut start = 0;
    while let Some(idx) = result[start..].find(pattern) {
        let actual_idx = start + idx;
        if let Some(end_idx) = result[actual_idx..].find('}') {
            let end = actual_idx + end_idx;
            let artifact_name = &result[actual_idx + pattern.len()..end];

            if let Some(path) = resolver
                .resolve_source_file(artifact_name)
                .or_else(|| resolver.resolve_build_artifact(artifact_name))
            {
                let path_str = path.to_string_lossy().to_string();
                result.replace_range(actual_idx..=end, &path_str);
                modified = true;
                start = actual_idx + path_str.len();
            } else {
                start = end + 1;
            }
        } else {
            break;
        }
    }
    modified
}

fn inject_string_placeholders(s: &mut String, resolver: &ArtifactResolver<'_>) -> bool {
    let mut result = s.clone();
    let mut modified = replace_ref_placeholders(&mut result, resolver);
    if replace_artifact_placeholders(&mut result, resolver) {
        modified = true;
    }
    if modified {
        *s = result;
    }
    modified
}

/// 递归地将产物路径注入到 JSON 值中
pub(crate) fn inject_paths_into_value(
    value: &mut serde_json::Value,
    resolver: &ArtifactResolver<'_>,
) -> bool {
    let mut modified = false;

    match value {
        serde_json::Value::String(s) => {
            if inject_string_placeholders(s, resolver) {
                modified = true;
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                if inject_paths_into_value(item, resolver) {
                    modified = true;
                }
            }
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                if inject_paths_into_value(v, resolver) {
                    modified = true;
                }
            }
        }
        _ => {}
    }

    modified
}

fn run_command_cwd_from_json_args(args: &str) -> Option<PathBuf> {
    let args_json = serde_json::from_str::<serde_json::Value>(args).ok()?;
    if let Some(cwd) = args_json.get("cwd").and_then(|v| v.as_str()) {
        return Some(PathBuf::from(cwd));
    }
    if let Some(command) = args_json.get("command").and_then(|v| v.as_str())
        && command == "cd"
        && let Some(dir) = args_json
            .get("args")
            .and_then(|a| a.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
    {
        return Some(PathBuf::from(dir));
    }
    None
}

fn run_command_cwd_from_make_output(output: &str) -> Option<PathBuf> {
    if !output.contains("make: 进入目录") && !output.contains("make: Entering directory") {
        return None;
    }
    let start = output
        .find("make: 进入目录\"")
        .or_else(|| output.find("make: Entering directory `"))?;
    let search_start = start + "make: 进入目录\"".len();
    let end = output[search_start..]
        .find('"')
        .or_else(|| output[search_start..].find('\''))?;
    let dir = &output[search_start..search_start + end];
    Some(PathBuf::from(dir))
}

fn cmake_build_dir_from_json_args(args: &str) -> Option<PathBuf> {
    let args_json = serde_json::from_str::<serde_json::Value>(args).ok()?;
    let build_dir = args_json.get("build_dir").and_then(|v| v.as_str())?;
    Some(PathBuf::from(build_dir))
}

/// 检测工作目录变化（从工具调用参数和输出中提取）
pub(crate) fn detect_working_dir_change(
    tool_call: &ToolCall,
    result: &ToolExecutionResult,
) -> Option<PathBuf> {
    let tool_name = &tool_call.function.name;
    let args = &tool_call.function.arguments;

    if tool_name == "run_command" {
        if let Some(p) = run_command_cwd_from_json_args(args) {
            return Some(p);
        }
        if let Some(p) = run_command_cwd_from_make_output(&result.output) {
            return Some(p);
        }
    }

    if tool_name == "cmake" {
        return cmake_build_dir_from_json_args(args);
    }

    None
}
