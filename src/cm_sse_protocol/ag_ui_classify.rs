//! AG-UI SSE `data:` 载荷分类（无 UI 回调；与 Web `parser_v2` 分发语义对齐）。

use serde_json::Value;

/// 单行或多行 AG-UI JSON 的分类结果（对应 Web `SseDispatch`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgUiParseDispatch {
    /// 已识别并应由消费方处理（含 CUSTOM / 工具事件等）。
    Handled,
    /// 非 AG-UI JSON 或未知 `type`：按纯文本 assistant delta 回落。
    Plain,
    /// 流式回合结束（`RUN_FINISHED` / `RUN_ERROR`）。
    StreamEnded,
}

/// 解析 `data:` 拼接块（可含多行 JSON），返回分类；**不**触发 UI 副作用。
pub fn classify_ag_ui_sse_data(data: &str) -> AgUiParseDispatch {
    let mut handled_any = false;
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<Value>(line) else {
            return AgUiParseDispatch::Plain;
        };
        let Some(type_str) = val.get("type").and_then(|v| v.as_str()) else {
            return AgUiParseDispatch::Plain;
        };
        handled_any = true;
        match type_str {
            "RUN_FINISHED" | "RUN_ERROR" => return AgUiParseDispatch::StreamEnded,
            "TOOL_CALL_START"
            | "TOOL_CALL_ARGS"
            | "TOOL_CALL_END"
            | "TOOL_CALL_RESULT"
            | "CUSTOM"
            | "TEXT_MESSAGE_CONTENT"
            | "REASONING_MESSAGE_CONTENT"
            | "STATE_SNAPSHOT" => {}
            "RUN_STARTED"
            | "TEXT_MESSAGE_START"
            | "TEXT_MESSAGE_END"
            | "REASONING_MESSAGE_START"
            | "REASONING_MESSAGE_END"
            | "STATE_DELTA" => {}
            _ => return AgUiParseDispatch::Plain,
        }
    }
    if handled_any {
        AgUiParseDispatch::Handled
    } else {
        AgUiParseDispatch::Plain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn golden_ag_ui_classify_matches_expected() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("fixtures/sse_ag_ui_golden.jsonl");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (line_no, line) in raw.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = t.splitn(3, '\t').collect();
            assert!(
                parts.len() == 3,
                "{}:{}: expected 3 tab columns, got {}",
                path.display(),
                line_no + 1,
                parts.len(),
            );
            let json_line = parts[1].trim();
            let want = parts[2].trim();
            let dispatch = classify_ag_ui_sse_data(json_line);
            let got = match dispatch {
                AgUiParseDispatch::Handled => "handled",
                AgUiParseDispatch::Plain => "plain",
                AgUiParseDispatch::StreamEnded => "stream_ended",
            };
            assert_eq!(
                got,
                want,
                "{}:{}: classify mismatch\n  desc: {}\n  json: {json_line}\n  want: {want}\n  got:  {got}",
                path.display(),
                line_no + 1,
                parts[0],
            );
        }
    }

    #[test]
    fn run_finished_is_stream_ended() {
        let data = r#"{"type":"RUN_FINISHED","threadId":"th-1","runId":"run-1"}"#;
        assert_eq!(
            classify_ag_ui_sse_data(data),
            AgUiParseDispatch::StreamEnded
        );
    }

    #[test]
    fn unknown_type_is_plain() {
        let data = r#"{"type":"UNKNOWN","foo":"bar"}"#;
        assert_eq!(classify_ag_ui_sse_data(data), AgUiParseDispatch::Plain);
    }

    #[test]
    fn non_json_is_plain() {
        assert_eq!(
            classify_ag_ui_sse_data("hello world"),
            AgUiParseDispatch::Plain
        );
    }
}
