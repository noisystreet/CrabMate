//! 产物占位符注入与工作目录探测。

use crate::types::ToolCall;

use super::super::artifact_resolver::ArtifactResolver;
use super::super::tool_executor::ToolExecutionResult;

/// 递归地将产物路径注入到 JSON 值中
pub(crate) fn inject_paths_into_value(
    value: &mut serde_json::Value,
    resolver: &ArtifactResolver<'_>,
) -> bool {
    let mut modified = false;

    match value {
        serde_json::Value::String(s) => {
            let mut result = s.clone();

            {
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
            }
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

            if modified {
                *s = result;
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

/// 检测工作目录变化（从工具调用参数和输出中提取）
pub(crate) fn detect_working_dir_change(
    tool_call: &ToolCall,
    result: &ToolExecutionResult,
) -> Option<std::path::PathBuf> {
    let tool_name = &tool_call.function.name;
    let args = &tool_call.function.arguments;

    if tool_name == "run_command" {
        if let Ok(args_json) = serde_json::from_str::<serde_json::Value>(args) {
            if let Some(cwd) = args_json.get("cwd").and_then(|v| v.as_str()) {
                return Some(std::path::PathBuf::from(cwd));
            }

            if let Some(command) = args_json.get("command").and_then(|v| v.as_str())
                && command == "cd"
                && let Some(dir) = args_json
                    .get("args")
                    .and_then(|a| a.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
            {
                return Some(std::path::PathBuf::from(dir));
            }
        }

        if (result.output.contains("make: 进入目录")
            || result.output.contains("make: Entering directory"))
            && let Some(start) = result
                .output
                .find("make: 进入目录\"")
                .or_else(|| result.output.find("make: Entering directory `"))
        {
            let search_start = start + "make: 进入目录\"".len();
            if let Some(end) = result.output[search_start..]
                .find('"')
                .or_else(|| result.output[search_start..].find('\''))
            {
                let dir = &result.output[search_start..search_start + end];
                return Some(std::path::PathBuf::from(dir));
            }
        }
    }

    if tool_name == "cmake"
        && let Ok(args_json) = serde_json::from_str::<serde_json::Value>(args)
        && let Some(build_dir) = args_json.get("build_dir").and_then(|v| v.as_str())
    {
        return Some(std::path::PathBuf::from(build_dir));
    }

    None
}
