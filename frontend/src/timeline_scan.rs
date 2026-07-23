//! 从当前会话消息中抽取「规划 / 工具」时间线条目（供侧栏式面板跳转）。

use serde::Deserialize;
use serde_json::json;

use crate::storage::{StoredMessage, StoredMessageState};

pub use crate::storage::TIMELINE_UI_STATE_KEY;

fn is_planner_tool_call_rejected_timeline_text(text: &str) -> bool {
    text.contains("planner_tool_call_rejected")
        || text.contains("PLANNER_TOOL_CALL_REJECTED")
        || text.contains("规划轮工具调用已拒绝")
}

/// 编排路由决议（`timeline_log` `kind=orchestration_route`）仅供 tracing / replay，不进聊天气泡。
fn is_orchestration_route_timeline_text(text: &str) -> bool {
    text.trim_start().starts_with("编排路由：")
}

/// 已落盘的编排路由旁注：导出与聊天列均跳过。
pub fn is_orchestration_route_timeline_message(m: &StoredMessage) -> bool {
    !m.is_tool && is_orchestration_route_timeline_text(&m.text)
}

/// 工具轮次前的 commentary 旁注（`commentary_before_tools`）：不进主气泡/导出。
pub fn is_commentary_before_tools_assistant(m: &StoredMessage) -> bool {
    m.role == "assistant"
        && !m.is_tool
        && m.state
            .as_ref()
            .is_some_and(|s| matches!(s, crate::storage::StoredMessageState::CommentaryBeforeTools))
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

fn is_bare_tool_path_arg_residue(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.lines().count() != 1 || t.contains(char::is_whitespace) {
        return false;
    }
    if t.contains("```") || t.contains('：') || t.contains(':') {
        return false;
    }
    let lower = t.to_lowercase();
    lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar")
        || lower.ends_with(".zip")
        || lower.ends_with(".gz")
        || is_bare_single_line_path_residue(t)
}

/// 单行、无扩展名或疑似截断的文件路径（如 `.../INSTALLe`）——多为 DSML/工具参数残留。
fn is_bare_single_line_path_residue(text: &str) -> bool {
    if !text.contains('/') {
        return false;
    }
    let basename = text.rsplit('/').next().unwrap_or(text);
    if basename.eq_ignore_ascii_case("Makefile") || basename.eq_ignore_ascii_case("README") {
        return false;
    }
    if basename.contains('.') {
        return false;
    }
    basename.len() <= 12
}
#[derive(Debug, Deserialize)]
struct TimelineUiState {
    k: String,
    t: String,
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

/// `final_response` 时间线补偿旁注：Phase 4 起不再 push 新行；保留供导出/水合单测与旧会话 state 解析。
#[allow(dead_code)]
pub fn timeline_state_final_response_snapshot() -> StoredMessageState {
    StoredMessageState::TimelineUiJson(
        json!({
            "k": TIMELINE_UI_STATE_KEY,
            "t": "final_response_snapshot",
            "msg": "",
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
    if is_orchestration_route_timeline_message(m) {
        return true;
    }
    if is_commentary_before_tools_assistant(m) {
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
    if is_bare_tool_path_arg_residue(&m.text) {
        return true;
    }
    is_timeline_snapshot_duplicate_of_canonical_assistant(m, session_messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{StoredMessage, StoredMessageState};

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
    fn parses_final_response_snapshot_without_msg_field() {
        let state =
            StoredMessageState::from_wire(r#"{"k":"cm_tl","t":"final_response_snapshot"}"#.into());
        assert_eq!(
            timeline_ui_snapshot_type(&state).as_deref(),
            Some("final_response_snapshot")
        );
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
            text: "规划轮工具调用已拒绝\ncode=PLANNER_TOOL_CALL_REJECTED".to_string(),
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
    fn commentary_before_tools_dropped_for_export() {
        let m = StoredMessage {
            id: "c1".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: "先写总结\n".into(),
            image_urls: vec![],
            state: Some(crate::storage::StoredMessageState::CommentaryBeforeTools),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        assert!(is_commentary_before_tools_assistant(&m));
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
    fn bare_archive_path_argument_residue_dropped_for_export() {
        let m = StoredMessage {
            id: "path-arg".into(),
            role: "assistant".into(),
            text: "hpcg-HPCG-release-3-1-0.tar.gz".into(),
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
    fn truncated_install_path_residue_dropped_for_export() {
        let m = StoredMessage {
            id: "install-trunc".into(),
            role: "assistant".into(),
            text: "hpcg-HPCG-release-3-1-0/INSTALLe".into(),
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
    fn makefile_path_residue_not_dropped_for_export() {
        let m = StoredMessage {
            id: "makefile-path".into(),
            role: "assistant".into(),
            text: "hpcg-HPCG-release-3-1-0/Makefile".into(),
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
