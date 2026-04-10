//! 从当前会话消息中抽取「规划 / 工具」时间线条目（供侧栏式面板跳转）。

use serde::Deserialize;
use serde_json::json;

use crate::message_format::STAGED_TIMELINE_SYSTEM_PREFIX;
use crate::storage::StoredMessage;

/// `StoredMessage.state` 内嵌 JSON 的判别键；**仅** Web 本地展示用，不参与模型上下文。
pub const TIMELINE_UI_STATE_KEY: &str = "cm_tl";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineKind {
    StagedStart {
        step_index: usize,
        total_steps: usize,
    },
    StagedEnd {
        step_index: usize,
        total_steps: usize,
        /// 原始 `staged_plan_step_finished.status`（如 `ok` / `failed`）。
        status: String,
    },
    Tool {
        /// `tool_result.ok`，缺省按成功展示。
        ok: bool,
    },
    /// 旧会话：仅有前缀与文案，无结构化 `state`。
    LegacyStaged,
    LegacyTool,
}

#[derive(Debug, Clone)]
pub struct TimelineEntry {
    pub message_id: String,
    pub kind: TimelineKind,
}

#[derive(Debug, Deserialize)]
struct TimelineUiState {
    k: String,
    t: String,
    msg: String,
    #[serde(default)]
    i: Option<u64>,
    #[serde(default)]
    n: Option<u64>,
    #[serde(default)]
    st: Option<String>,
    #[serde(default)]
    ok: Option<bool>,
}

fn status_is_failed(status: &str) -> bool {
    matches!(status.trim(), "failed" | "失败")
}

/// 是否应在时间线中高亮为失败态（分阶段步结束或工具返回 `ok: false`）。
pub fn timeline_entry_is_failed(kind: &TimelineKind) -> bool {
    match kind {
        TimelineKind::StagedEnd { status, .. } => status_is_failed(status),
        TimelineKind::Tool { ok } => !*ok,
        _ => false,
    }
}

fn parse_ui_state(raw: &str) -> Option<TimelineUiState> {
    let v: TimelineUiState = serde_json::from_str(raw).ok()?;
    if v.k != TIMELINE_UI_STATE_KEY {
        return None;
    }
    Some(v)
}

/// 扫描单条消息；不匹配则返回 `None`。
pub fn timeline_entry_for_message(m: &StoredMessage) -> Option<TimelineEntry> {
    if let Some(ref st) = m.state {
        if let Some(u) = parse_ui_state(st) {
            let id = if u.msg.is_empty() {
                m.id.clone()
            } else {
                u.msg.clone()
            };
            return match u.t.as_str() {
                "staged_start" => Some(TimelineEntry {
                    message_id: id,
                    kind: TimelineKind::StagedStart {
                        step_index: u.i.unwrap_or(0) as usize,
                        total_steps: u.n.unwrap_or(0) as usize,
                    },
                }),
                "staged_end" => Some(TimelineEntry {
                    message_id: id,
                    kind: TimelineKind::StagedEnd {
                        step_index: u.i.unwrap_or(0) as usize,
                        total_steps: u.n.unwrap_or(0) as usize,
                        status: u.st.unwrap_or_default(),
                    },
                }),
                "tool" => Some(TimelineEntry {
                    message_id: id,
                    kind: TimelineKind::Tool {
                        ok: u.ok.unwrap_or(true),
                    },
                }),
                _ => None,
            };
        }
    }

    if m.role == "system"
        && m.text.starts_with(STAGED_TIMELINE_SYSTEM_PREFIX)
        && m.state.as_deref() != Some("error")
    {
        return Some(TimelineEntry {
            message_id: m.id.clone(),
            kind: TimelineKind::LegacyStaged,
        });
    }

    if m.is_tool && m.role == "system" && m.state.as_deref() != Some("error") {
        return Some(TimelineEntry {
            message_id: m.id.clone(),
            kind: TimelineKind::LegacyTool,
        });
    }

    None
}

pub fn collect_timeline_entries(messages: &[StoredMessage]) -> Vec<TimelineEntry> {
    messages
        .iter()
        .filter_map(timeline_entry_for_message)
        .collect()
}

/// 写入 `StoredMessage.state` 的 JSON，供时间线面板解析（**仅**本机 UI）。
pub fn timeline_state_staged_start(msg_id: &str, step_index: usize, total_steps: usize) -> String {
    json!({
        "k": TIMELINE_UI_STATE_KEY,
        "t": "staged_start",
        "msg": msg_id,
        "i": step_index,
        "n": total_steps,
    })
    .to_string()
}

pub fn timeline_state_staged_end(
    msg_id: &str,
    step_index: usize,
    total_steps: usize,
    status: &str,
) -> String {
    json!({
        "k": TIMELINE_UI_STATE_KEY,
        "t": "staged_end",
        "msg": msg_id,
        "i": step_index,
        "n": total_steps,
        "st": status,
    })
    .to_string()
}

pub fn timeline_state_tool(msg_id: &str, ok: bool) -> String {
    json!({
        "k": TIMELINE_UI_STATE_KEY,
        "t": "tool",
        "msg": msg_id,
        "ok": ok,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessage;

    #[test]
    fn parses_staged_end_failed() {
        let m = StoredMessage {
            id: "m1".into(),
            role: "system".into(),
            text: "### x".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(
                r#"{"k":"cm_tl","t":"staged_end","msg":"m1","i":2,"n":5,"st":"failed"}"#.into(),
            ),
            is_tool: false,
            created_at: 0,
        };
        let e = timeline_entry_for_message(&m).expect("entry");
        assert!(timeline_entry_is_failed(&e.kind));
    }

    #[test]
    fn tool_not_ok_highlighted() {
        let m = StoredMessage {
            id: "t1".into(),
            role: "system".into(),
            text: "tool".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(r#"{"k":"cm_tl","t":"tool","msg":"t1","ok":false}"#.into()),
            is_tool: true,
            created_at: 0,
        };
        let e = timeline_entry_for_message(&m).expect("entry");
        assert!(timeline_entry_is_failed(&e.kind));
    }
}
