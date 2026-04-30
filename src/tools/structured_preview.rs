//! 从工具原始文本输出中提取 **SSE `tool_result.structured_preview`** 用的 JSON。
//!
//! 约定：若干只读文件工具在正文前输出**单行** `crabmate_tool_output` JSON（与 `read_file` 一致），
//! 便于 Web/集成方解析元数据而不依赖正则扫 `output`。

use serde_json::Value;

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

/// 为 `emit_sse_tool_result` 生成可选的结构化预览对象（体积须小；**不**含文件正文）。
pub fn structured_preview_for_tool_sse(tool_name: &str, result: &str) -> Option<Value> {
    match tool_name {
        "read_file" | "read_dir" | "list_tree" => crabmate_tool_output_header(tool_name, result),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let p = structured_preview_for_tool_sse("read_file", &combined).expect("preview");
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
        let p = structured_preview_for_tool_sse("list_tree", &combined).expect("preview");
        assert_eq!(p["tool"], "list_tree");
        assert_eq!(p["lines_count"], 5);
    }

    #[test]
    fn wrong_tool_yields_none() {
        let s = "{\"kind\":\"crabmate_tool_output\",\"tool\":\"read_file\",\"version\":1}\n";
        assert!(structured_preview_for_tool_sse("read_dir", s).is_none());
    }
}
