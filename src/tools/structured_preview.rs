//! 从工具原始文本输出中提取 **SSE `tool_result.structured_preview`** 用的 JSON。
//!
//! 约定：若干只读文件工具在正文前输出**单行** `crabmate_tool_output` JSON（与 `read_file` 一致），
//! 便于 Web/集成方解析元数据而不依赖正则扫 `output`。
//!
//! **`run_command`**：若 **`标准输出：`** 段为类 **`git diff`** 的 unified diff（以 **`diff --git`**
//! 或 **`diff --cc`** 起首），或 **`git status`**（命令行解析为 **`git … status`** 且 stdout 符合常见
//! **`git status`** 形态），则附加与写工具相同的 **`preview: workspace_write_diff`** 头，
//! Web 工具卡复用既有 diff 预览块（见 `frontend` `workspace_write_diff_section`）。

use serde_json::{Value, json};

use super::write_sse_preview::MAX_UNIFIED_PREVIEW_FILE_CHARS;

/// 若 `result` 首行是 `{"kind":"crabmate_tool_output","tool":…}` 且 `tool` 与 `tool_name` 一致，返回该 JSON；否则 `None`。
pub fn crabmate_tool_output_header(tool_name: &str, result: &str) -> Option<Value> {
    let first = result.lines().next()?.trim();
    if first.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(first).ok()?;
    let obj = v.as_object()?;
    if obj.get("kind").and_then(|k| k.as_str()) != Some("crabmate_tool_output") {
        return None;
    }
    if obj.get("tool").and_then(|t| t.as_str()) != Some(tool_name) {
        return None;
    }
    Some(v)
}

/// 合并 **`crabmate_tool_output`** 首行预览与信封 **`structured_payload`**（如 **`run_command`**），供 SSE **`tool_result.structured_preview`** 单一出口。
pub fn merge_sse_structured_preview(
    tool_name: &str,
    result: &str,
    envelope_structured: Option<&Value>,
) -> Option<Value> {
    let header = crabmate_tool_output_header(tool_name, result);
    match (header, envelope_structured) {
        (None, None) => None,
        (Some(h), None) => Some(h),
        (None, Some(p)) => Some(p.clone()),
        (Some(h), Some(p)) => {
            let mut m = serde_json::Map::new();
            m.insert("tool_output_header".into(), h);
            m.insert("structured_payload".into(), p.clone());
            Some(Value::Object(m))
        }
    }
}

/// 为 `emit_sse_tool_result` 生成可选的结构化预览对象（体积须小；**不**含文件正文）。
pub fn structured_preview_for_tool_sse(
    tool_name: &str,
    result: &str,
    envelope_structured: Option<&Value>,
) -> Option<Value> {
    merge_sse_structured_preview(tool_name, result, envelope_structured)
}

fn first_non_empty_line(s: &str) -> Option<&str> {
    s.lines().map(str::trim).find(|line| !line.is_empty())
}

fn stdout_looks_like_git_unified_diff(stdout: &str) -> bool {
    let Some(line) = first_non_empty_line(stdout) else {
        return false;
    };
    line.starts_with("diff --git ") || line.starts_with("diff --cc ")
}

fn invocation_is_git_status(invocation: &str) -> bool {
    let tokens: Vec<&str> = invocation.split_whitespace().collect();
    let Some(git_pos) = tokens.iter().position(|&t| t == "git") else {
        return false;
    };
    let mut i = git_pos + 1;
    while i < tokens.len() {
        let t = tokens[i];
        if t == "-C" || t == "--git-dir" {
            i = i.saturating_add(2);
            continue;
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        break;
    }
    tokens.get(i).copied() == Some("status")
}

fn stdout_looks_like_git_status(stdout: &str) -> bool {
    let Some(first) = first_non_empty_line(stdout) else {
        return false;
    };
    if first.starts_with("On branch ")
        || first.starts_with("## ")
        || first.starts_with("HEAD detached")
        || first.starts_with("位于分支")
        || first == "Not currently on any branch."
    {
        return true;
    }
    let bytes = first.as_bytes();
    if bytes.len() >= 3 && bytes[2] == b' ' {
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        const STATUS_XY: &str = "MADRCU?! ";
        if STATUS_XY.contains(x) && STATUS_XY.contains(y) {
            return true;
        }
    }
    stdout.contains("Changes not staged for commit")
        || stdout.contains("Changes to be committed")
        || stdout.contains("Untracked files:")
        || stdout.contains("nothing to commit, working tree clean")
        || stdout.contains("无文件要提交")
        || stdout.contains("尚未暂存以备提交的变更")
}

fn run_command_stdout_qualifies_for_workspace_preview(stdout: &str, invocation: &str) -> bool {
    stdout_looks_like_git_unified_diff(stdout)
        || (invocation_is_git_status(invocation) && stdout_looks_like_git_status(stdout))
}

fn git_unified_diff_display_path(stdout: &str, invocation: &str) -> String {
    if let Some(line) = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("diff --git "))
        && let Some(rest) = line.trim_start().strip_prefix("diff --git ")
    {
        let mut parts = rest.split_whitespace();
        if let Some(_a) = parts.next()
            && let Some(b_path) = parts.next()
        {
            let b_path = b_path.trim_matches('"');
            let b_path = b_path.strip_prefix("b/").unwrap_or(b_path);
            if !b_path.is_empty() {
                return b_path.to_string();
            }
        }
    }
    let inv = invocation.trim();
    if inv.is_empty() {
        return "git diff".to_string();
    }
    let cap = 120usize;
    if inv.chars().count() > cap {
        let head: String = inv.chars().take(cap).collect();
        format!("{head}…")
    } else {
        inv.to_string()
    }
}

fn git_status_preview_label(invocation: &str) -> String {
    let inv = invocation.trim();
    if inv.is_empty() {
        return "git status".to_string();
    }
    let cap = 120usize;
    if inv.chars().count() > cap {
        let head: String = inv.chars().take(cap).collect();
        format!("{head}…")
    } else {
        inv.to_string()
    }
}

fn workspace_preview_display_path(stdout: &str, invocation: &str) -> String {
    if stdout_looks_like_git_unified_diff(stdout) {
        git_unified_diff_display_path(stdout, invocation)
    } else {
        git_status_preview_label(invocation)
    }
}

fn truncate_stdout_for_preview(stdout: &str) -> (String, bool) {
    let n = stdout.chars().count();
    if n <= MAX_UNIFIED_PREVIEW_FILE_CHARS {
        return (stdout.to_string(), false);
    }
    let head: String = stdout
        .chars()
        .take(MAX_UNIFIED_PREVIEW_FILE_CHARS)
        .collect();
    (format!("{head}…\n（该文件 diff 已截断）"), true)
}

/// 若 **`stdout`** 为类 **`git diff`** 的 unified diff，或 **`git status`** 且输出形态匹配，则并入
/// **`workspace_write_diff`** 预览头，与 Web 端 `structured_preview.tool_output_header` / 根级 **`preview`** 解析一致。
pub(crate) fn augment_run_command_preview_with_git_diff(
    preview: Option<Value>,
    raw_output: &str,
    stdout: &str,
) -> Option<Value> {
    let invocation = raw_output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("命令："))
        .map(str::trim)
        .unwrap_or("");
    if !run_command_stdout_qualifies_for_workspace_preview(stdout, invocation) {
        return preview;
    }
    let display_path = workspace_preview_display_path(stdout, invocation);
    let (unified_diff, trunc_file) = truncate_stdout_for_preview(stdout);
    let tool_header = json!({
        "kind": "crabmate_tool_output",
        "tool": "run_command",
        "version": 1_u32,
        "preview": "workspace_write_diff",
        "files": [{
            "path": display_path,
            "unified_diff": unified_diff,
            "truncated": trunc_file,
        }],
        "preview_truncated": trunc_file,
    });

    match preview {
        None => Some(tool_header),
        Some(v) => {
            if let Some(h) = v.get("tool_output_header")
                && h.get("preview").and_then(|p| p.as_str()) == Some("workspace_write_diff")
            {
                return Some(v);
            }
            if v.get("preview").and_then(|p| p.as_str()) == Some("workspace_write_diff") {
                return Some(v);
            }
            if v.get("kind").and_then(|k| k.as_str()) == Some("crabmate_structured_payload") {
                return Some(json!({
                    "tool_output_header": tool_header,
                    "structured_payload": v,
                }));
            }
            if let Some(obj) = v.as_object()
                && obj.contains_key("structured_payload")
                && !obj.contains_key("tool_output_header")
            {
                let mut m = obj.clone();
                m.insert("tool_output_header".into(), tool_header);
                return Some(Value::Object(m));
            }
            Some(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preview_header_and_payload() {
        let header = "{\"kind\":\"crabmate_tool_output\",\"tool\":\"read_file\",\"version\":1,\"path\":\"a.rs\"}\nbody";
        let payload = serde_json::json!({"kind":"crabmate_structured_payload","tool":"run_command","version":1});
        let m = crate::tools::structured_preview::merge_sse_structured_preview(
            "read_file",
            header,
            Some(&payload),
        )
        .expect("merged");
        assert!(m.get("tool_output_header").is_some());
        assert!(m.get("structured_payload").is_some());
    }

    #[test]
    fn read_file_header_roundtrip() {
        let header = serde_json::json!({
            "kind": "crabmate_tool_output",
            "tool": "read_file",
            "version": 1,
            "path": "src/x.rs",
            "start_line": 1,
            "end_line_shown": 5,
            "line_count_returned": 5,
            "total_lines": 100,
            "truncated_by_max_lines": false,
            "has_more": false,
            "file_empty": false,
        });
        let body = "line1\nline2";
        let combined = format!("{}\n{}", header, body);
        let p = structured_preview_for_tool_sse("read_file", &combined, None).expect("preview");
        assert_eq!(p["tool"], "read_file");
        assert_eq!(p["path"], "src/x.rs");
    }

    #[test]
    fn list_tree_header_roundtrip() {
        let header = serde_json::json!({
            "kind": "crabmate_tool_output",
            "tool": "list_tree",
            "version": 1,
            "path": "src",
            "max_depth": 2,
            "max_entries": 100,
            "include_hidden": false,
            "lines_count": 5,
            "truncated": false,
        });
        let body = "dir: .\nfile: a.rs\n";
        let combined = format!("{}\n{}", header, body);
        let p = structured_preview_for_tool_sse("list_tree", &combined, None).expect("preview");
        assert_eq!(p["tool"], "list_tree");
        assert_eq!(p["lines_count"], 5);
    }

    #[test]
    fn wrong_tool_yields_none() {
        let s = "{\"kind\":\"crabmate_tool_output\",\"tool\":\"read_file\",\"version\":1}\n";
        assert!(structured_preview_for_tool_sse("read_dir", s, None).is_none());
    }

    #[test]
    fn run_command_git_diff_attaches_workspace_write_diff_header() {
        let payload = serde_json::json!({
            "kind": "crabmate_structured_payload",
            "tool": "run_command",
            "version": 1_u64,
            "schema": "run_command_exit_v1",
            "invocation": "git diff",
            "ok": true,
            "exit_code": 0,
            "stdout_nonempty": true,
            "stderr_nonempty": false,
        });
        let stdout = "diff --git a/README.md b/README.md\nindex 111..222 100644\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-x\n+y\n";
        let out = augment_run_command_preview_with_git_diff(
            Some(payload.clone()),
            "命令：git diff\n",
            stdout,
        )
        .expect("augmented");
        let h = out.get("tool_output_header").expect("tool_output_header");
        assert_eq!(h["preview"], "workspace_write_diff");
        assert_eq!(h["tool"], "run_command");
        let files = h["files"].as_array().expect("files");
        assert_eq!(files[0]["path"], "README.md");
        assert!(
            files[0]["unified_diff"]
                .as_str()
                .is_some_and(|s| s.contains("diff --git"))
        );
        assert_eq!(out["structured_payload"], payload);
    }

    #[test]
    fn run_command_git_status_attaches_workspace_write_diff_header() {
        let payload = serde_json::json!({
            "kind": "crabmate_structured_payload",
            "tool": "run_command",
            "version": 1_u64,
            "schema": "run_command_exit_v1",
            "invocation": "git status",
            "ok": true,
            "exit_code": 0,
            "stdout_nonempty": true,
            "stderr_nonempty": false,
        });
        let stdout = "On branch main\nnothing to commit, working tree clean\n";
        let out = augment_run_command_preview_with_git_diff(
            Some(payload.clone()),
            "命令：git status\n",
            stdout,
        )
        .expect("augmented");
        let h = out.get("tool_output_header").expect("tool_output_header");
        assert_eq!(h["preview"], "workspace_write_diff");
        let files = h["files"].as_array().expect("files");
        assert_eq!(files[0]["path"], "git status");
        assert!(
            files[0]["unified_diff"]
                .as_str()
                .is_some_and(|s| s.contains("On branch main"))
        );
        assert_eq!(out["structured_payload"], payload);
    }

    #[test]
    fn run_command_git_status_short_branch_porcelain_preview() {
        let payload = serde_json::json!({
            "kind": "crabmate_structured_payload",
            "tool": "run_command",
            "version": 1_u64,
            "schema": "run_command_exit_v1",
            "invocation": "git status -sb",
            "ok": true,
            "exit_code": 0,
            "stdout_nonempty": true,
            "stderr_nonempty": false,
        });
        let stdout = "## main...origin/main [ahead 1]\n M README.md\n";
        let out = augment_run_command_preview_with_git_diff(
            Some(payload.clone()),
            "命令：git status -sb\n",
            stdout,
        )
        .expect("augmented");
        let h = out.get("tool_output_header").expect("tool_output_header");
        let files = h["files"].as_array().expect("files");
        assert_eq!(files[0]["path"], "git status -sb");
        assert!(
            files[0]["unified_diff"]
                .as_str()
                .is_some_and(|s| s.contains("## main"))
        );
    }

    #[test]
    fn run_command_non_diff_stdout_leaves_preview_unchanged() {
        let payload = serde_json::json!({
            "kind": "crabmate_structured_payload",
            "tool": "run_command",
            "version": 1_u64,
            "schema": "run_command_exit_v1",
            "invocation": "echo hi",
            "ok": true,
            "exit_code": 0,
            "stdout_nonempty": true,
            "stderr_nonempty": false,
        });
        let stdout = "hello\n";
        let out = augment_run_command_preview_with_git_diff(
            Some(payload.clone()),
            "命令：echo hi\n",
            stdout,
        );
        assert_eq!(out, Some(payload));
    }
}
