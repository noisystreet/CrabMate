//! 解析持久化的 `{"crabmate_tool":{...}}` 工具消息，供水合 / 展示 / 导出与 SSE `tool_card_*` 对齐。

use serde_json::Value;

use crate::i18n::Locale;
use crate::sse_dispatch::ToolResultInfo;
use crate::storage::StoredMessage;

use super::stored_message::tool_stored_text_from_envelope;

/// 是否为 `role=tool` 落盘的 `crabmate_tool` 信封 JSON。
#[must_use]
pub fn looks_like_crabmate_tool_envelope(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with('{') && t.contains("\"crabmate_tool\"")
}

/// 从存储正文解析 [`ToolResultInfo`]；`fallback_name` 为 API `name` 字段（与信封内 `name` 互补）。
pub fn tool_result_info_from_stored_content(
    raw: &str,
    fallback_name: Option<&str>,
) -> Option<ToolResultInfo> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let ct = v.get("crabmate_tool")?.as_object()?;
    let name = ct
        .get("name")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| {
            fallback_name
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        })?;
    Some(ToolResultInfo {
        name,
        goal_id: None,
        tool_call_id: ct
            .get("tool_call_id")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        result_version: ct.get("v").and_then(|x| x.as_u64()).unwrap_or(1) as u32,
        summary: ct
            .get("summary")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        output: ct
            .get("output")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        ok: ct.get("ok").and_then(|x| x.as_bool()),
        exit_code: ct.get("exit_code").and_then(|x| x.as_i64()),
        error_code: ct
            .get("error_code")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        failure_category: ct
            .get("failure_category")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        structured_preview: ct.get("structured_payload").cloned(),
    })
}

/// 水合 `role=tool` 时格式化为与 SSE `on_tool_result` 一致的 `(compact, detail)`。
pub fn format_tool_role_content_for_stored_message(
    raw: &str,
    fallback_name: Option<&str>,
    loc: Locale,
) -> Option<(String, String)> {
    tool_stored_text_from_envelope(raw, fallback_name, loc).map(|t| (t.compact, t.detail))
}

/// 工具气泡紧凑行：已格式化则原样；否则尝试解析信封。
#[must_use]
pub fn stored_tool_message_compact_text(m: &StoredMessage, loc: Locale) -> String {
    if !m.is_tool {
        return m.text.clone();
    }
    let compact = m.text.trim();
    if !compact.is_empty() && !looks_like_crabmate_tool_envelope(compact) {
        return m.text.clone();
    }
    if let Some((c, _)) =
        format_tool_role_content_for_stored_message(compact, m.tool_name.as_deref(), loc)
    {
        return c;
    }
    m.text.clone()
}

/// 工具气泡详情 / 导出正文：优先已格式化的 `reasoning_text`；否则解析 `text` 信封。
#[must_use]
pub fn stored_tool_message_detail_text(m: &StoredMessage, loc: Locale) -> String {
    if !m.is_tool {
        return m.reasoning_text.clone();
    }
    let detail = m.reasoning_text.trim();
    if !detail.is_empty() && !looks_like_crabmate_tool_envelope(detail) {
        return m.reasoning_text.clone();
    }
    let raw = if looks_like_crabmate_tool_envelope(m.text.as_str()) {
        m.text.as_str()
    } else if looks_like_crabmate_tool_envelope(detail) {
        detail
    } else if !detail.is_empty() {
        return m.reasoning_text.clone();
    } else if !m.text.trim().is_empty() {
        m.text.as_str()
    } else {
        return String::new();
    };
    format_tool_role_content_for_stored_message(raw, m.tool_name.as_deref(), loc)
        .map(|(_, d)| d)
        .unwrap_or_else(|| {
            if !detail.is_empty() {
                m.reasoning_text.clone()
            } else {
                m.text.clone()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessage;
    use crate::timeline_scan::timeline_state_tool;

    const GIT_STATUS_ENVELOPE: &str = r#"{"crabmate_tool":{"execution_mode":"parallel_readonly_batch","exit_code":0,"name":"git_status","ok":true,"output":"git status (exit=0):\n位于分支 main","parallel_batch_id":"prb-1","summary":"git status","tool_call_id":"call_test","v":1}}"#;

    #[test]
    fn parses_git_status_envelope() {
        let info = tool_result_info_from_stored_content(GIT_STATUS_ENVELOPE, None).unwrap();
        assert_eq!(info.name, "git_status");
        assert_eq!(info.summary.as_deref(), Some("git status"));
        assert!(info.ok.unwrap());
    }

    #[test]
    fn formats_hydrated_tool_message_for_display() {
        let m = StoredMessage {
            id: "t".into(),
            role: "system".into(),
            text: GIT_STATUS_ENVELOPE.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_tool("t", true)),
            is_tool: true,
            tool_call_id: None,
            tool_name: Some("git_status".into()),
            created_at: 0,
        };
        let compact = stored_tool_message_compact_text(&m, Locale::ZhHans);
        let detail = stored_tool_message_detail_text(&m, Locale::ZhHans);
        assert!(!compact.contains("crabmate_tool"), "compact={compact:?}");
        assert!(!detail.contains("crabmate_tool"), "detail={detail:?}");
        assert!(compact.contains("git_status") || compact.contains("git status"));
        assert!(detail.contains("位于分支 main"));
    }

    #[test]
    fn preserves_sse_formatted_tool_rows() {
        let m = StoredMessage {
            id: "t".into(),
            role: "system".into(),
            text: "git_status · git status".into(),
            reasoning_text: "tool: git_status\ngit status (exit=0):\nok".into(),
            image_urls: vec![],
            state: Some(timeline_state_tool("t", true)),
            is_tool: true,
            tool_call_id: None,
            tool_name: Some("git_status".into()),
            created_at: 0,
        };
        assert_eq!(
            stored_tool_message_compact_text(&m, Locale::ZhHans),
            "git_status · git status"
        );
        assert_eq!(
            stored_tool_message_detail_text(&m, Locale::ZhHans),
            "tool: git_status\ngit status (exit=0):\nok"
        );
    }
}
