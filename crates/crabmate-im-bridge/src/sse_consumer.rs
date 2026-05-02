//! 消费 CrabMate **`POST /chat/stream`** 的 SSE 文本流：解析 `data:` 行、累积终答、识别工具审批请求。

use crabmate_sse_protocol::{
    classify_sse_control_outcome, join_sse_data_lines, parse_sse_event_id,
};
use serde_json::Value;

/// 从 SSE 文本缓冲中取出完整事件块（以空行分隔）。
pub fn take_complete_sse_blocks(buf: &mut String) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(pos) = buf.find("\n\n") {
        let block = buf[..pos].to_string();
        *buf = buf[pos + 2..].to_string();
        if !block.trim().is_empty() {
            out.push(block);
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct StreamAccum {
    /// 终答正文（`classify_sse_control_outcome` 为 `plain` 的片段拼接）。
    pub answer: String,
    pub saw_error: bool,
    pub error_preview: String,
}

#[derive(Debug, Clone)]
pub struct CommandApprovalNotice {
    pub command: String,
    pub args: String,
}

/// 解析单个 `data:` 负载：更新 `acc`，并返回本帧内的审批请求与状态行。
fn process_one_sse_data_payload(
    data: &str,
    acc: &mut StreamAccum,
) -> (Vec<CommandApprovalNotice>, Vec<String>) {
    let mut approvals = Vec::new();
    let mut status_lines = Vec::new();

    let t = data.trim();
    if t.is_empty() || t == "[DONE]" {
        return (approvals, status_lines);
    }

    let v: Value = match serde_json::from_str(t) {
        Ok(v) => v,
        Err(_) => {
            acc.answer.push_str(data);
            return (approvals, status_lines);
        }
    };

    let outcome = classify_sse_control_outcome(&v);
    match outcome {
        "stop" => {
            acc.saw_error = true;
            let msg = v.get("error").and_then(|x| x.as_str()).unwrap_or("error");
            let code = v.get("code").and_then(|x| x.as_str()).unwrap_or("");
            acc.error_preview = if code.is_empty() {
                msg.to_string()
            } else {
                format!("{msg} ({code})")
            };
        }
        "handled" => {
            if let Some(obj) = v
                .get("command_approval_request")
                .and_then(|x| x.as_object())
            {
                let command = obj
                    .get("command")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = obj
                    .get("args")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                approvals.push(CommandApprovalNotice { command, args });
            }
            if let Some(obj) = v.get("timeline_log").and_then(|x| x.as_object()) {
                let kind = obj.get("kind").and_then(|x| x.as_str()).unwrap_or("");
                let title = obj.get("title").and_then(|x| x.as_str()).unwrap_or("");
                let detail = obj
                    .get("detail")
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty());
                let line = match detail {
                    Some(d) => format!("[{kind}] {title}: {d}"),
                    None => format!("[{kind}] {title}"),
                };
                status_lines.push(line);
            }
            if let Some(obj) = v.get("tool_call").and_then(|x| x.as_object()) {
                let name = obj.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                let summary = obj.get("summary").and_then(|x| x.as_str()).unwrap_or("");
                let line = if summary.is_empty() {
                    format!("🔧 工具: {name}")
                } else {
                    format!("🔧 工具: {name} — {summary}")
                };
                status_lines.push(line);
            }
            if v.get("tool_running") == Some(&Value::Bool(true)) {
                status_lines.push("⏳ 正在执行工具…".into());
            }
            if v.get("parsing_tool_calls") == Some(&Value::Bool(true)) {
                status_lines.push("🧩 正在解析工具调用…".into());
            }
        }
        _ => {
            acc.answer.push_str(data);
        }
    }
    (approvals, status_lines)
}

/// 同单帧解析结果：更新 `acc`，并返回本帧内的审批请求与状态行。
pub fn handle_sse_data_payload_collect(
    data: &str,
    acc: &mut StreamAccum,
) -> (Vec<CommandApprovalNotice>, Vec<String>) {
    process_one_sse_data_payload(data, acc)
}

/// 将完整 SSE 事件块解析为负载后交给 [`handle_sse_data_payload_collect`]。
pub fn dispatch_sse_event_block_collect(
    block: &str,
    acc: &mut StreamAccum,
) -> (Vec<CommandApprovalNotice>, Vec<String>) {
    let _id = parse_sse_event_id(block);
    let Some(payload) = join_sse_data_lines(block) else {
        return (Vec::new(), Vec::new());
    };
    handle_sse_data_payload_collect(&payload, acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_double_crlf_blocks() {
        let mut buf = "id: 1\ndata: {\"v\":1}\n\n".to_string();
        let b = take_complete_sse_blocks(&mut buf);
        assert_eq!(b.len(), 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn accumulates_plain_after_control() {
        let mut acc = StreamAccum::default();
        let (approvals, _) = handle_sse_data_payload_collect(
            r#"{"v":1,"command_approval_request":{"command":"run_command","args":"ls"}}"#,
            &mut acc,
        );
        assert_eq!(approvals.len(), 1);
        let (_, _) = handle_sse_data_payload_collect("hello", &mut acc);
        assert_eq!(acc.answer, "hello");
    }
}
