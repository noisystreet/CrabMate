//! 节点输出占位符注入（`{{node.output}}` 等）。

use std::collections::HashMap;

use super::types::{NodeRunResult, NodeRunStatus};

pub(crate) fn inject_placeholders(
    value: &serde_json::Value,
    completed: &HashMap<String, NodeRunResult>,
    max_chars: usize,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            serde_json::Value::String(inject_string(s, completed, max_chars))
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|v| inject_placeholders(v, completed, max_chars))
                .collect(),
        ),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), inject_placeholders(v, completed, max_chars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn inject_string(s: &str, completed: &HashMap<String, NodeRunResult>, max_chars: usize) -> String {
    let mut out = String::new();
    let mut rest = s;
    loop {
        let start = match rest.find("{{") {
            Some(i) => i,
            None => {
                out.push_str(rest);
                break;
            }
        };
        let (prefix, tail) = rest.split_at(start);
        out.push_str(prefix);
        let end = match tail.find("}}") {
            Some(i) => i,
            None => {
                // 没有闭合，直接把剩余内容追加
                out.push_str(tail);
                break;
            }
        };
        let inner = tail[2..end].trim(); // skip {{
        let replacement = resolve_placeholder(inner, completed, max_chars);
        out.push_str(&replacement);
        // move past }}
        rest = &tail[end + 2..];
    }
    out
}

fn resolve_placeholder(
    inner: &str,
    completed: &HashMap<String, NodeRunResult>,
    max_chars: usize,
) -> String {
    // 支持：
    // - {{node_id.output}}
    // - {{node_id.status}}
    // - {{node_id.stdout_first_line}}
    // 未来可扩展更多字段。
    let parts: Vec<&str> = inner.split('.').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return String::new();
    }

    let node_id = parts[0];
    if let Some(r) = completed.get(node_id) {
        let field = if parts.len() == 2 { parts[1] } else { parts[2] };
        match field {
            "output" => truncate_for_injection(&r.output, max_chars),
            "status" => match r.status {
                NodeRunStatus::Passed => "passed".to_string(),
                NodeRunStatus::Failed => "failed".to_string(),
            },
            "stdout_first_line" => r
                .output
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .chars()
                .take(max_chars)
                .collect::<String>(),
            "stdout_first_token" => r
                .output
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .chars()
                .take(max_chars)
                .collect::<String>(),
            _ => String::new(),
        }
    } else {
        String::new()
    }
}

fn truncate_for_injection(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}... (截断)", &s[..max_chars])
    }
}
