//! Replay runner：从 SSE 事件录制文件或后端 turn-replay 事件文件读取事件，
//! 映射为 [`TurnEvent`] 并驱动 [`TurnReducer`] + [`project_turn_web`] 投影。
//!
//! 支持两种 JSONL 格式（自动检测）：
//! 1. **`sse-replay-events.jsonl`**（由 `CM_SSE_REPLAY_DUMP_DIR` 生成）：每行含 `seq`/`job_id`/`data`，`data` 为 AG-UI JSON。
//! 2. **`turn-replay-events.jsonl`**（由 `CM_REPLAY_DUMP_DIR` 生成）：后端决策级事件，含 `event`/`detail`/`seq`/`job_id` 等字段。
//!
//! 用法：
//! ```ignore
//! crabmate sse-replay sse-replay-events.jsonl
//! crabmate sse-replay turn-replay-events.jsonl --format canonical
//! ```

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::event::TurnEvent;
use crate::model::{SegmentKind, Turn};
use crate::project::project_turn_web;
use crate::reduce::TurnReducer;

// ── 格式 1：sse-replay-events.jsonl ──────────────────────────────

/// SSE replay 行外壳。
#[derive(Debug, serde::Deserialize)]
struct SseReplayLine {
    #[allow(dead_code)]
    seq: u64,
    #[allow(dead_code)]
    job_id: u64,
    /// V2Encoder 编码后的 AG-UI JSON 字符串。
    data: String,
}

// ── 格式 2：turn-replay-events.jsonl ──────────────────────────────

/// 后端 turn-replay 行外壳。
#[derive(Debug, serde::Deserialize)]
struct TurnReplayLine {
    #[allow(dead_code)]
    seq: u64,
    #[allow(dead_code)]
    job_id: u64,
    event: String,
    #[serde(default)]
    detail: serde_json::Value,
    #[serde(default)]
    #[expect(dead_code, reason = "反序列化占位")]
    title: String,
    #[allow(dead_code)]
    #[serde(default)]
    replay_turn_seq: u64,
}

/// 去掉 markdown code fence 包裹（```json ... ``` / ``` ... ```）。
fn strip_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with("```json") {
        trimmed
            .strip_prefix("```json")
            .unwrap_or(trimmed)
            .trim_end_matches('`')
            .trim()
            .to_string()
    } else if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```")
            .unwrap_or(trimmed)
            .trim_end_matches('`')
            .trim()
            .to_string()
    } else {
        trimmed.to_string()
    }
}

/// 判断 `assistant_content` 是否为意图分析 JSON。
fn is_intent_analysis_json(text: &str) -> bool {
    let stripped = strip_code_fence(text);
    !stripped.is_empty()
        && stripped.starts_with('{')
        && stripped.contains("\"kind\"")
        && stripped.contains("\"primary_intent\"")
}

/// 从 `assistant_content` 提取 TurnEvent（意图分析 → TimelineAssistant；其他忽略）。
/// 非意图分析的 assistant_content 不再产生事件
/// （终答由 overlay 承载，不经过 canonical reducer）。
fn assistant_content_to_events(text: &str) -> Vec<TurnEvent> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    if is_intent_analysis_json(text) {
        vec![TurnEvent::TimelineAssistant {
            text: text.to_string(),
        }]
    } else {
        Vec::new()
    }
}

/// 将后端 turn-replay 事件映射为 0 或多个 [`TurnEvent`]。
fn map_turn_replay_event_to_turn_events(line: &TurnReplayLine) -> Vec<TurnEvent> {
    match line.event.as_str() {
        "llm_response_done" => map_llm_response_done(&line.detail),
        _ => Vec::new(),
    }
}

fn map_llm_response_done(det: &serde_json::Value) -> Vec<TurnEvent> {
    let mut out = Vec::new();

    // tool_calls → ToolCall 事件
    let tool_calls = det.get("tool_calls").and_then(|v| v.as_array());
    let has_tools = tool_calls.is_some_and(|tcs| !tcs.is_empty());

    if let Some(tcs) = tool_calls {
        for tc in tcs {
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tool_call_id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                out.push(TurnEvent::ToolCall {
                    tool_call_id,
                    name,
                    summary: String::new(),
                });
            }
        }
    }

    // 如果本次 LLM 响应无工具调用，先发 ToolPhaseEnd（关闭工具阶段）
    if !has_tools {
        out.push(TurnEvent::ToolPhaseEnd);
    }

    // assistant_content → TimelineAssistant（意图分析时）
    // 非意图分析的正文不再产生 canonical 事件
    if let Some(text) = det.get("assistant_content").and_then(|v| v.as_str()) {
        out.extend(assistant_content_to_events(text));
    }

    out
}

// ── SSE 事件解析 ──────────────────────────────────────────────────

/// 从 CUSTOM turn_segment_start 提取 SegmentStart 事件。
fn parse_segment_start(data: &serde_json::Value) -> Option<TurnEvent> {
    let segment_id = data
        .get("segmentId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let kind_str = data
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("commentary");
    let kind = match kind_str {
        "answer" => SegmentKind::Answer,
        _ => SegmentKind::Commentary,
    };
    let before_tool_call_id = data
        .get("beforeToolCallId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    Some(TurnEvent::SegmentStart {
        segment_id,
        kind,
        before_tool_call_id,
    })
}

/// 从 CUSTOM turn_segment_end 提取 SegmentEnd 事件。
fn parse_segment_end(data: &serde_json::Value) -> Option<TurnEvent> {
    let segment_id = data
        .get("segmentId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(TurnEvent::SegmentEnd { segment_id })
}

/// 从 CUSTOM timeline_log 提取事件。
fn parse_timeline_log(data: &serde_json::Value) -> Option<TurnEvent> {
    let kind = data.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "intent_analysis" | "approval_decision" | "tool_result_summary" => {
            let text = data
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if text.is_empty() {
                None
            } else {
                Some(TurnEvent::TimelineAssistant { text })
            }
        }
        "final_response" => {
            // final_response 不再产生 canonical 事件
            None
        }
        _ => None,
    }
}

/// 将单个 AG-UI SSE JSON 映射为 0 或多个 [`TurnEvent`]。
fn map_single_sse_value(val: &serde_json::Value) -> Vec<TurnEvent> {
    let Some(type_str) = val.get("type").and_then(|v| v.as_str()) else {
        return Vec::new();
    };

    match type_str {
        "TEXT_MESSAGE_CONTENT" => {
            // 纯文本 delta 不再产生 canonical 事件
            Vec::new()
        }
        "TOOL_CALL_START" => {
            let name = val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tool_call_id = val
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                Vec::new()
            } else {
                vec![TurnEvent::ToolCall {
                    tool_call_id,
                    name,
                    summary: String::new(),
                }]
            }
        }
        "CUSTOM" => {
            let custom_type = val.get("customType").and_then(|v| v.as_str());
            let data = val.get("data");
            match custom_type {
                Some("turn_segment_start") => {
                    data.and_then(parse_segment_start).into_iter().collect()
                }
                Some("turn_segment_end") => data.and_then(parse_segment_end).into_iter().collect(),
                Some("turn_tool_phase_end") => vec![TurnEvent::ToolPhaseEnd],
                Some("timeline_log") => data.and_then(parse_timeline_log).into_iter().collect(),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// 将 AG-UI SSE `data` 行解析为 0 或多个 [`TurnEvent`]。
fn map_sse_data_to_turn_events(data: &str) -> Vec<TurnEvent> {
    let mut out = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            out.extend(map_single_sse_value(&val));
        }
        // 非 JSON 行不产生 canonical 事件
    }
    out
}

// ── 自动检测格式 ──────────────────────────────────────────────────

/// JSONL 行格式检测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonlFormat {
    /// `sse-replay-events.jsonl`：含 `data` 字段
    SseReplay,
    /// `turn-replay-events.jsonl`：含 `event` 字段
    TurnReplay,
}

fn detect_format(first_line: &str) -> Result<JsonlFormat, String> {
    let val: serde_json::Value =
        serde_json::from_str(first_line).map_err(|e| format!("JSON 解析失败: {e}"))?;
    if val.get("data").is_some() {
        Ok(JsonlFormat::SseReplay)
    } else if val.get("event").is_some() {
        Ok(JsonlFormat::TurnReplay)
    } else {
        Err("无法识别 JSONL 格式：缺少 `data` 或 `event` 字段".to_string())
    }
}

// ── 公共接口 ──────────────────────────────────────────────────────

/// 从 JSONL 事件文件读取事件（自动检测格式），回放为 Web 块布局投影行。
///
/// 对于多 Turn 文件，合并所有 Turn 的投影行（按 Turn 顺序排列）。
pub fn replay_sse_events_to_web_rows(
    path: &Path,
) -> Result<Vec<crate::project::ProjectedRow>, String> {
    let turns = replay_all_turns(path)?;
    let mut rows = Vec::new();
    for (i, turn) in turns.iter().enumerate() {
        if turns.len() > 1 {
            rows.push(crate::project::ProjectedRow {
                kind: "turn_marker".to_string(),
                text: format!("── Turn {} ──", i + 1),
                tool_name: None,
                tool_call_id: None,
            });
        }
        rows.extend(project_turn_web(turn));
    }
    Ok(rows)
}

/// 从 JSONL 事件文件读取事件（自动检测格式），回放为 [`Turn`] 并返回 canonical 状态。
///
/// 对于 `turn-replay-events.jsonl` 格式，按 `replay_turn_seq` 分裂为多个独立 Turn，
/// 返回最后一个 Turn 的状态。若需查看所有 Turn，使用 [`replay_all_turns`]。
pub fn replay_sse_events_to_turn(path: &Path) -> Result<Turn, String> {
    let all = replay_all_turns(path)?;
    all.into_iter()
        .last()
        .ok_or_else(|| "JSONL 文件中无有效 Turn 事件".to_string())
}

/// 处理单行 JSONL 事件。
fn process_replay_line(
    line: &str,
    format: JsonlFormat,
    current_turn_seq: &mut u64,
    current_turn: &mut Turn,
    turns: &mut Vec<Turn>,
    reducer: &TurnReducer,
) -> Result<usize, String> {
    let events = match format {
        JsonlFormat::SseReplay => {
            let replay_line: SseReplayLine = serde_json::from_str(line)
                .map_err(|e| format!("SSE replay JSON 解析失败: {e}\n  行: {line}"))?;
            map_sse_data_to_turn_events(&replay_line.data)
        }
        JsonlFormat::TurnReplay => {
            let replay_line: TurnReplayLine = serde_json::from_str(line)
                .map_err(|e| format!("Turn replay JSON 解析失败: {e}\n  行: {line}"))?;
            let this_seq = replay_line.replay_turn_seq;
            if this_seq != *current_turn_seq && this_seq > 0 {
                if *current_turn_seq > 0 {
                    crate::close_open_commentary_segments(current_turn);
                    turns.push(std::mem::take(current_turn));
                }
                *current_turn_seq = this_seq;
            }
            map_turn_replay_event_to_turn_events(&replay_line)
        }
    };

    let count = events.len();
    for ev in events {
        reducer.apply(current_turn, ev);
    }
    Ok(count)
}

/// 从 JSONL 事件文件读取事件，回放所有 Turn，按 `replay_turn_seq` 分裂。
///
/// 对于 SSE replay 格式（无 `replay_turn_seq`），所有事件归入单个 Turn。
pub fn replay_all_turns(path: &Path) -> Result<Vec<Turn>, String> {
    let file = File::open(path).map_err(|e| format!("无法打开 {}: {e}", path.display()))?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines().peekable();

    // 用首行检测格式
    let format = {
        let first = lines.peek().ok_or_else(|| "JSONL 文件为空".to_string())?;
        let first_line = first.as_ref().map_err(|e| format!("读取首行失败: {e}"))?;
        detect_format(first_line.trim())?
    };

    let mut turns: Vec<Turn> = Vec::new();
    let mut current_turn = Turn::default();
    let mut current_turn_seq: u64 = 0;
    let reducer = TurnReducer;
    let mut event_count = 0usize;

    for line_result in lines {
        let line = line_result.map_err(|e| format!("读取行失败: {e}"))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        event_count += process_replay_line(
            line,
            format,
            &mut current_turn_seq,
            &mut current_turn,
            &mut turns,
            &reducer,
        )?;
    }

    // 最后一个 Turn
    if event_count > 0 {
        crate::close_open_commentary_segments(&mut current_turn);
        turns.push(current_turn);
    }

    log::info!(
        target: "crabmate-turn-layout",
        "Replay ({format:?}): {event_count} TurnEvents → {} turns",
        turns.len(),
    );
    Ok(turns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_text_message_content_produces_no_event() {
        // 纯文本 delta 不再产生 canonical 事件
        let data = r#"{"type":"TEXT_MESSAGE_CONTENT","delta":"你好"}"#;
        let events = map_sse_data_to_turn_events(data);
        assert!(events.is_empty());
    }

    #[test]
    fn map_turn_segment_start_to_segment_start() {
        let data = r#"{"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-before-tc1","kind":"commentary","beforeToolCallId":"tc1"}}"#;
        let events = map_sse_data_to_turn_events(data);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            TurnEvent::SegmentStart {
                segment_id: "seg-before-tc1".to_string(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: Some("tc1".to_string()),
            }
        );
    }

    #[test]
    fn map_timeline_log_final_response_produces_no_event() {
        // final_response 不再产生 canonical 事件
        let data = r#"{"type":"CUSTOM","customType":"timeline_log","data":{"kind":"final_response","title":"终答","detail":"已完成创建。"}}"#;
        let events = map_sse_data_to_turn_events(data);
        assert!(events.is_empty());
    }

    #[test]
    fn map_tool_call_start_to_tool_call() {
        let data = r#"{"type":"TOOL_CALL_START","toolCallId":"tc-1","name":"read_file","parentMessageId":"msg-1"}"#;
        let events = map_sse_data_to_turn_events(data);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            TurnEvent::ToolCall {
                tool_call_id: "tc-1".to_string(),
                name: "read_file".to_string(),
                summary: String::new(),
            }
        );
    }

    #[test]
    fn replay_sse_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sse-replay-events.jsonl");
        let jsonl = r#"{"seq":1,"job_id":1,"data":"{\"type\":\"CUSTOM\",\"customType\":\"turn_segment_start\",\"data\":{\"segmentId\":\"seg-1\",\"kind\":\"commentary\",\"beforeToolCallId\":null}}"}
{"seq":2,"job_id":1,"data":"{\"type\":\"CUSTOM\",\"customType\":\"turn_segment_end\",\"data\":{\"segmentId\":\"seg-1\"}}"}
{"seq":3,"job_id":1,"data":"{\"type\":\"CUSTOM\",\"customType\":\"turn_tool_phase_end\",\"data\":{\"phase\":\"tool_end\"}}"}
{"seq":4,"job_id":1,"data":"{\"type\":\"TEXT_MESSAGE_CONTENT\",\"delta\":\"完成。\"}"}
"#;
        std::fs::write(&path, jsonl).expect("write jsonl");
        let rows = replay_sse_events_to_web_rows(&path).expect("replay");
        // 无工具调用场景：`project_turn_web` 仅产生 batch 行（纯文本 delta 不写入 canonical）
        // 终答由 overlay 承载，replay 不产生 `assistant_answer` 行
        assert!(
            rows.is_empty() || rows.iter().any(|r| r.kind == "assistant_batch_narration"),
            "expected batch or empty rows, got: {rows:?}"
        );
    }

    #[test]
    fn replay_turn_replay_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("turn-replay-events.jsonl");
        let jsonl = r#"{"seq":1,"job_id":1,"event":"llm_response_done","detail":{"llm_call_id":"llm-1","assistant_content":"","tool_calls":[{"id":"tc-1","function":{"name":"create_file"}}]},"title":"llm-1"}
{"seq":2,"job_id":1,"event":"tool_call_finished","detail":{"tool_call_id":"tc-1","name":"create_file"},"title":"create_file"}
{"seq":3,"job_id":1,"event":"llm_response_done","detail":{"llm_call_id":"llm-2","assistant_content":"完成。","tool_calls":[]},"title":"llm-2"}
"#;
        std::fs::write(&path, jsonl).expect("write jsonl");
        let rows = replay_sse_events_to_web_rows(&path).expect("replay");
        // `project_turn_web` 不产生 `assistant_answer` 行
        assert!(
            rows.iter().any(|r| r.kind == "tool"),
            "expected tool row, got: {rows:?}"
        );
    }

    #[test]
    fn detect_format_sse_vs_turn() {
        let sse_line = r#"{"seq":1,"job_id":1,"data":"{}"}"#;
        let turn_line = r#"{"seq":1,"job_id":1,"event":"llm_response_done","detail":{}}"#;
        assert_eq!(detect_format(sse_line).unwrap(), JsonlFormat::SseReplay);
        assert_eq!(detect_format(turn_line).unwrap(), JsonlFormat::TurnReplay);
    }
}
