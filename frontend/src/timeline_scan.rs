//! 从当前会话消息中抽取「规划 / 工具」时间线条目（供侧栏式面板跳转）。

use serde::Deserialize;
use serde_json::json;

use crate::message_format::STAGED_TIMELINE_SYSTEM_PREFIX;
use crate::storage::{StoredMessage, StoredMessageState};

pub use crate::storage::TIMELINE_UI_STATE_KEY;

fn is_planner_tool_call_rejected_timeline_text(text: &str) -> bool {
    text.starts_with(STAGED_TIMELINE_SYSTEM_PREFIX)
        && (text.contains("planner_tool_call_rejected")
            || text.contains("PLANNER_TOOL_CALL_REJECTED")
            || text.contains("规划轮工具调用已拒绝"))
}

fn json_value_looks_like_tool_args(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Array(a) => {
            let mut saw_arg_like = false;
            for x in a {
                let Some(s) = x.as_str() else {
                    return false;
                };
                let t = s.trim();
                saw_arg_like |= t.starts_with('-')
                    || t.contains('=')
                    || t.contains('/')
                    || t.ends_with(".gz")
                    || t.ends_with(".tar");
            }
            !a.is_empty() && saw_arg_like
        }
        serde_json::Value::Object(o) => o.keys().any(|k| {
            matches!(
                k.as_str(),
                "command" | "args" | "path" | "content" | "cmd" | "pattern" | "file" | "files"
            )
        }),
        _ => false,
    }
}

fn is_tool_argument_residue_assistant_text(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(t).is_ok_and(|v| json_value_looks_like_tool_args(&v))
}

fn is_bare_shell_command_residue(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.lines().count() != 1 {
        return false;
    }
    let lower = t.to_lowercase();
    let command_like = [
        "tar ", "make ", "cmake ", "bash ", "sh ", "ls ", "cat ", "python ", "python3 ", "./",
        "cargo ",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));
    command_like
        && (lower.contains(" -")
            || lower.contains(" --")
            || lower.contains('/')
            || lower.contains(".tar")
            || lower.contains(".gz")
            || lower.contains('='))
}

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
    ApprovalDecision {
        /// `allow` | `deny`。
        decision: String,
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
        if let Some(raw) = st.as_timeline_parse_candidate()
            && let Some(u) = parse_ui_state(raw)
        {
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
                "approval_decision" => Some(TimelineEntry {
                    message_id: id,
                    kind: TimelineKind::ApprovalDecision {
                        decision: u.st.unwrap_or_else(|| "allow".to_string()),
                    },
                }),
                _ => None,
            };
        }
    }

    if m.role == "system"
        && m.text.starts_with(STAGED_TIMELINE_SYSTEM_PREFIX)
        && !m.state.as_ref().is_some_and(|s| s.is_error())
    {
        return Some(TimelineEntry {
            message_id: m.id.clone(),
            kind: TimelineKind::LegacyStaged,
        });
    }

    if m.is_tool && m.role == "system" && !m.state.as_ref().is_some_and(|s| s.is_error()) {
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

/// 写入 `StoredMessage.state` 的时间线 JSON，供侧栏解析（**仅**本机 UI）。
pub fn timeline_state_staged_start(
    msg_id: &str,
    step_index: usize,
    total_steps: usize,
) -> StoredMessageState {
    StoredMessageState::TimelineUiJson(
        json!({
            "k": TIMELINE_UI_STATE_KEY,
            "t": "staged_start",
            "msg": msg_id,
            "i": step_index,
            "n": total_steps,
        })
        .to_string(),
    )
}

pub fn timeline_state_staged_end(
    msg_id: &str,
    step_index: usize,
    total_steps: usize,
    status: &str,
) -> StoredMessageState {
    StoredMessageState::TimelineUiJson(
        json!({
            "k": TIMELINE_UI_STATE_KEY,
            "t": "staged_end",
            "msg": msg_id,
            "i": step_index,
            "n": total_steps,
            "st": status,
        })
        .to_string(),
    )
}

pub fn timeline_state_tool(msg_id: &str, ok: bool) -> StoredMessageState {
    StoredMessageState::TimelineUiJson(
        json!({
            "k": TIMELINE_UI_STATE_KEY,
            "t": "tool",
            "msg": msg_id,
            "ok": ok,
        })
        .to_string(),
    )
}

/// 本地时间线快照（仅用于 hydrate 保留；不会进入侧栏时间线条目）。
pub fn timeline_state_local_snapshot() -> StoredMessageState {
    StoredMessageState::TimelineUiJson(
        json!({
            "k": TIMELINE_UI_STATE_KEY,
            "t": "local_snapshot",
        })
        .to_string(),
    )
}

/// 意图分析旁注：流式期间展示，水合后保留，并随会话导出。
pub fn timeline_state_intent_analysis_snapshot() -> StoredMessageState {
    StoredMessageState::TimelineUiJson(
        json!({
            "k": TIMELINE_UI_STATE_KEY,
            "t": "intent_analysis",
        })
        .to_string(),
    )
}

/// `final_response` 时间线补偿旁注：正文已在流式助手或服务端快照中时不再保留。
pub fn timeline_state_final_response_snapshot() -> StoredMessageState {
    StoredMessageState::TimelineUiJson(
        json!({
            "k": TIMELINE_UI_STATE_KEY,
            "t": "final_response_snapshot",
        })
        .to_string(),
    )
}

fn parse_timeline_ui_snapshot_type(raw: &str) -> Option<String> {
    let v: TimelineUiState = serde_json::from_str(raw).ok()?;
    if v.k != TIMELINE_UI_STATE_KEY {
        return None;
    }
    Some(v.t)
}

/// 从 [`StoredMessageState`] 读取时间线快照 `t`（如 `intent_analysis`）。
pub fn timeline_ui_snapshot_type(state: &StoredMessageState) -> Option<String> {
    state
        .as_timeline_parse_candidate()
        .and_then(parse_timeline_ui_snapshot_type)
}

fn server_assistant_has_trimmed_text(server_msgs: &[StoredMessage], text: &str) -> bool {
    let needle = text.trim();
    if needle.is_empty() {
        return false;
    }
    server_msgs
        .iter()
        .any(|m| m.role == "assistant" && !m.is_tool && m.text.trim() == needle)
}

/// 本地 timeline 旁注是否与同会话内「正式」助手行正文重复（兼容旧 `local_snapshot`）。
pub fn is_timeline_snapshot_duplicate_of_canonical_assistant(
    m: &StoredMessage,
    session_messages: &[StoredMessage],
) -> bool {
    if m.role != "assistant" || m.is_tool {
        return false;
    }
    let Some(state) = m.state.as_ref() else {
        return false;
    };
    if !state.is_local_timeline_snapshot_row() {
        return false;
    }
    let needle = m.text.trim();
    if needle.is_empty() {
        return false;
    }
    session_messages.iter().any(|other| {
        other.id != m.id
            && other.role == "assistant"
            && !other.is_tool
            && !other
                .state
                .as_ref()
                .is_some_and(|s| s.is_local_timeline_snapshot_row())
            && other.text.trim() == needle
    })
}

/// 水合合并时：`final_response` 补偿旁注若与服务端助手正文重复则丢弃。
pub fn should_preserve_local_timeline_on_hydrate(
    m: &StoredMessage,
    server_msgs: &[StoredMessage],
) -> bool {
    let Some(state) = m.state.as_ref() else {
        return false;
    };
    if !state.is_local_timeline_snapshot_row() {
        return false;
    }
    match timeline_ui_snapshot_type(state).as_deref() {
        Some("final_response_snapshot") => !server_assistant_has_trimmed_text(server_msgs, &m.text),
        Some("intent_analysis") => !server_assistant_has_trimmed_text(server_msgs, &m.text),
        Some("local_snapshot") | None => {
            !server_assistant_has_trimmed_text(server_msgs, &m.text)
                && !is_timeline_snapshot_duplicate_of_canonical_assistant(m, server_msgs)
        }
        _ => true,
    }
}

/// 会话导出时跳过仅用于流式 UI 的助手旁注（`final_response` 补偿、重复旧快照等）。
pub fn is_ephemeral_timeline_assistant_for_export(
    m: &StoredMessage,
    session_messages: &[StoredMessage],
) -> bool {
    if m.role != "assistant" || m.is_tool {
        return false;
    }
    if m.state
        .as_ref()
        .and_then(timeline_ui_snapshot_type)
        .is_some_and(|t| t == "final_response_snapshot")
    {
        return true;
    }
    if is_planner_tool_call_rejected_timeline_text(&m.text) {
        return true;
    }
    if crate::message_format::stored_message_is_staged_planner_round(m) {
        return true;
    }
    if is_tool_argument_residue_assistant_text(&m.text) {
        return true;
    }
    if is_bare_shell_command_residue(&m.text) {
        return true;
    }
    is_timeline_snapshot_duplicate_of_canonical_assistant(m, session_messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{StoredMessage, StoredMessageState};

    #[test]
    fn parses_staged_end_failed() {
        let m = StoredMessage {
            id: "m1".into(),
            role: "system".into(),
            text: "### x".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::from_wire(
                r#"{"k":"cm_tl","t":"staged_end","msg":"m1","i":2,"n":5,"st":"failed"}"#.into(),
            )),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
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
            state: Some(StoredMessageState::from_wire(
                r#"{"k":"cm_tl","t":"tool","msg":"t1","ok":false}"#.into(),
            )),
            is_tool: true,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        let e = timeline_entry_for_message(&m).expect("entry");
        assert!(timeline_entry_is_failed(&e.kind));
    }

    #[test]
    fn intent_analysis_preserved_for_export_and_hydrate() {
        let m = StoredMessage {
            id: "i1".into(),
            role: "assistant".into(),
            text: "意图分析：执行类\n\n".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_intent_analysis_snapshot()),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        assert!(!is_ephemeral_timeline_assistant_for_export(&m, &[]));
        assert!(should_preserve_local_timeline_on_hydrate(&m, &[]));
    }

    #[test]
    fn intent_analysis_not_preserved_when_server_has_same_text() {
        let intent_text = "意图分析：问答类（直接回复）\n\n";
        let local = StoredMessage {
            id: "local-intent".into(),
            role: "assistant".into(),
            text: intent_text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_intent_analysis_snapshot()),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 1,
        };
        let server = vec![StoredMessage {
            id: "srv-intent".into(),
            role: "assistant".into(),
            text: intent_text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 1,
        }];
        assert!(!should_preserve_local_timeline_on_hydrate(&local, &server));
    }

    #[test]
    fn legacy_local_snapshot_dropped_when_server_has_same_assistant_text() {
        let snap = StoredMessage {
            id: "legacy-fr".into(),
            role: "assistant".into(),
            text: "same answer".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_local_snapshot()),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        let server = vec![StoredMessage {
            id: "srv".into(),
            role: "assistant".into(),
            text: "same answer".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 1,
        }];
        assert!(!should_preserve_local_timeline_on_hydrate(&snap, &server));
        assert!(is_ephemeral_timeline_assistant_for_export(
            &snap,
            &[snap.clone(), server[0].clone()]
        ));
    }

    #[test]
    fn final_response_snapshot_dropped_when_server_has_same_text() {
        let snap = StoredMessage {
            id: "fr1".into(),
            role: "assistant".into(),
            text: "hello world".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_final_response_snapshot()),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        let server = vec![StoredMessage {
            id: "srv".into(),
            role: "assistant".into(),
            text: "hello world".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 1,
        }];
        assert!(is_ephemeral_timeline_assistant_for_export(&snap, &server));
        assert!(!should_preserve_local_timeline_on_hydrate(&snap, &server));
    }

    #[test]
    fn planner_tool_call_rejected_timeline_dropped_for_export() {
        let m = StoredMessage {
            id: "reject".into(),
            role: "assistant".into(),
            text: format!(
                "{}规划轮工具调用已拒绝\ncode=PLANNER_TOOL_CALL_REJECTED",
                STAGED_TIMELINE_SYSTEM_PREFIX
            ),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_local_snapshot()),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        assert!(is_ephemeral_timeline_assistant_for_export(&m, &[]));
    }

    #[test]
    fn staged_planner_round_dropped_for_export() {
        let m = StoredMessage {
            id: "plan".into(),
            role: "assistant".into(),
            text: r#"1. `rerun-demo`: 重新运行 demo

```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"rerun-demo","description":"重新运行 demo"}],"no_task":false}
```"#
                .into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        assert!(is_ephemeral_timeline_assistant_for_export(&m, &[]));
    }

    #[test]
    fn tool_argument_array_residue_dropped_for_export() {
        let m = StoredMessage {
            id: "args".into(),
            role: "assistant".into(),
            text: r#"["-c","tar xzf hpcg-HPCG-release-3-1-0.tar.gz"]"#.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        assert!(is_ephemeral_timeline_assistant_for_export(&m, &[]));
    }

    #[test]
    fn bare_shell_command_residue_dropped_for_export() {
        let m = StoredMessage {
            id: "cmd".into(),
            role: "assistant".into(),
            text: "tar -xzf hpcg-HPCG-release-3-1-0.tar.gz".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        assert!(is_ephemeral_timeline_assistant_for_export(&m, &[]));
    }

    #[test]
    fn explanatory_shell_command_answer_is_kept_for_export() {
        let m = StoredMessage {
            id: "cmd-explain".into(),
            role: "assistant".into(),
            text: "可以执行：\n\n```bash\ntar -xzf archive.tar.gz\n```".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        assert!(!is_ephemeral_timeline_assistant_for_export(&m, &[]));
    }
}
