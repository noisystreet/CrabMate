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
}
