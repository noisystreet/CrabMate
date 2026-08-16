use serde_json::Value;

/// 从单个 SSE block 中解析 `id: <u64>`。
pub fn parse_sse_event_id(block: &str) -> Option<u64> {
    for line in block.lines() {
        let t = line.trim_start();
        let rest = t.strip_prefix("id:")?;
        let s = rest.trim();
        if let Ok(n) = s.parse::<u64>() {
            return Some(n);
        }
    }
    None
}

/// 拼接单个 SSE block 中的全部 `data: ` 行（保留 payload 前导空格与换行）。
pub fn join_sse_data_lines(block: &str) -> Option<String> {
    let data_lines: Vec<&str> = block.lines().filter(|l| l.starts_with("data: ")).collect();
    if data_lines.is_empty() {
        return None;
    }
    Some(
        data_lines
            .iter()
            .map(|l| &l[6..])
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// SSE 约定的尾帧哨兵（兼容前后空白）。
pub fn is_sse_done_sentinel(data: &str) -> bool {
    data.trim() == "[DONE]"
}

/// 从 JSON `data` 中提取 `stream_ended.reason` 原始字符串。
pub fn extract_stream_ended_reason(data: &str) -> Option<String> {
    let v: Value = serde_json::from_str(data).ok()?;
    let obj = v.as_object()?;
    let ended = obj.get("stream_ended")?.as_object()?;
    let reason = ended.get("reason")?.as_str()?;
    Some(reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        extract_stream_ended_reason, is_sse_done_sentinel, join_sse_data_lines, parse_sse_event_id,
    };

    #[test]
    fn parse_id_and_data_lines() {
        let block = "id: 42\ndata: {\"k\":1}\ndata: next";
        assert_eq!(parse_sse_event_id(block), Some(42));
        assert_eq!(
            join_sse_data_lines(block).as_deref(),
            Some("{\"k\":1}\nnext")
        );
    }

    #[test]
    fn stream_ended_reason_extracts() {
        let data = "{\"stream_ended\":{\"job_id\":1,\"reason\":\"completed\"}}";
        assert_eq!(
            extract_stream_ended_reason(data).as_deref(),
            Some("completed")
        );
    }

    #[test]
    fn done_sentinel_trimmed() {
        assert!(is_sse_done_sentinel(" [DONE]\n"));
        assert!(!is_sse_done_sentinel("DONE"));
    }
}
