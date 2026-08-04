//! 侧栏会话过滤与跨会话消息全文搜索（本地内存扫描，不建持久索引）。

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

use crate::i18n::Locale;
use crate::storage::ChatSession;
use crate::stream_text_overlay::StreamTextOverlay;
use crate::stream_text_overlay::message_text_for_display_including_stream_overlay;

/// 规范化查询：小写、折叠空白。
pub fn normalize_search_query(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// 会话标题是否匹配（小写子串）。
pub fn session_title_matches(session: &ChatSession, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    session.title.to_lowercase().contains(needle_lower)
}

/// 单条消息搜索命中（跨会话列表展示用）。
#[derive(Debug, Clone)]
pub struct MessageSearchHit {
    pub session_id: String,
    pub session_title: String,
    pub message_id: String,
    pub snippet: String,
}

const SNIPPET_MAX_CHARS: usize = 140;
const SNIPPET_CONTEXT: usize = 28;
/// 全局消息搜索最多条数，避免超大会话卡 UI。
pub const MESSAGE_SEARCH_MAX_HITS: usize = 80;

/// 在所有本地会话的消息展示文本中搜索（大小写不敏感）。
///
/// `stream_overlay`：与 [`crate::chat_session_state::ChatSessionSignals::stream_text_overlay`] 当前快照一致时，
/// 尾条 `loading` 助手的流式增量会参与匹配（与主列气泡、会话内查找同源）。
pub fn collect_message_search_hits(
    sessions: &[ChatSession],
    needle_lower: &str,
    max_hits: usize,
    loc: Locale,
    apply_assistant_display_filters: bool,
    stream_overlay: Option<&StreamTextOverlay>,
) -> Vec<MessageSearchHit> {
    if needle_lower.is_empty() || max_hits == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for s in sessions {
        for m in &s.messages {
            let display = message_text_for_display_including_stream_overlay(
                m,
                stream_overlay,
                s.id.as_str(),
                loc,
                apply_assistant_display_filters,
            );
            let lower = display.to_lowercase();
            if lower.contains(needle_lower) {
                out.push(MessageSearchHit {
                    session_id: s.id.clone(),
                    session_title: s.title.clone(),
                    message_id: m.id.clone(),
                    snippet: snippet_around_match(&display, needle_lower, SNIPPET_MAX_CHARS),
                });
                if out.len() >= max_hits {
                    return out;
                }
            }
        }
    }
    out
}

fn snippet_around_match(hay: &str, needle_lower: &str, max_chars: usize) -> String {
    let lower = hay.to_lowercase();
    let Some(pos_byte) = lower.find(needle_lower) else {
        return trim_snippet_chars(hay, max_chars);
    };
    let match_start = hay[..pos_byte].chars().count();
    let win_start = match_start.saturating_sub(SNIPPET_CONTEXT);
    let inner: String = hay.chars().skip(win_start).take(max_chars).collect();
    let has_more_after = hay.chars().count() > win_start + inner.chars().count();
    let mut out = String::new();
    if win_start > 0 {
        out.push('…');
    }
    out.push_str(&inner);
    if has_more_after {
        out.push('…');
    }
    out
}

fn trim_snippet_chars(s: &str, max: usize) -> String {
    let mut t: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        t.push('…');
    }
    t
}

/// `id="msg-{…}"` 片段仅允许安全字符（与 `make_session_id` / `make_message_id` 生成一致）。
pub fn is_safe_dom_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

/// 将消息滚入主消息区可视范围（仅 WASM）。
///
/// 优先匹配 TUI 主列 `section.chat-tui-turn[data-tui-msg-id]`；兼容旧气泡 `id="msg-{id}"`。
#[cfg(target_arch = "wasm32")]
pub fn scroll_message_into_view(msg_id: &str) {
    if !is_safe_dom_token(msg_id) {
        return;
    }
    let Some(win) = web_sys::window() else {
        return;
    };
    let Some(doc) = win.document() else {
        return;
    };
    let tui_sel = format!("section.chat-tui-turn[data-tui-msg-id=\"{msg_id}\"]");
    let el = doc
        .query_selector(&tui_sel)
        .ok()
        .flatten()
        .or_else(|| doc.get_element_by_id(&format!("msg-{msg_id}")));
    let Some(el) = el else {
        return;
    };
    if let Ok(he) = el.dyn_into::<web_sys::HtmlElement>() {
        he.scroll_into_view();
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn scroll_message_into_view(msg_id: &str) {
    let _ = is_safe_dom_token(msg_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use crate::storage::StoredMessage;

    fn sess(id: &str, title: &str, messages: Vec<StoredMessage>) -> ChatSession {
        ChatSession {
            id: id.to_string(),
            layout_schema_version: crate::storage::CURRENT_LAYOUT_SCHEMA_VERSION,
            title: title.to_string(),
            draft: String::new(),
            messages,
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        }
    }

    #[test]
    fn session_title_filter() {
        let s = sess("a", "Hello 世界", vec![]);
        assert!(session_title_matches(&s, ""));
        assert!(session_title_matches(&s, "hello"));
        assert!(session_title_matches(&s, "世界"));
        assert!(!session_title_matches(&s, "zzz"));
    }

    #[test]
    fn dom_token_allows_message_ids() {
        assert!(is_safe_dom_token("s_123_456"));
        assert!(!is_safe_dom_token(""));
        assert!(!is_safe_dom_token("x\"y"));
    }

    #[test]
    fn message_hits_across_sessions() {
        let sessions = vec![
            sess(
                "s1",
                "A",
                vec![StoredMessage {
                    id: "m1".into(),
                    role: "user".into(),
                    text: "alpha beta gamma".into(),
                    reasoning_text: String::new(),
                    image_urls: vec![],
                    state: None,
                    is_tool: false,
                    tool_call_id: None,
                    tool_name: None,
                    created_at: 0,
                }],
            ),
            sess(
                "s2",
                "B",
                vec![StoredMessage {
                    id: "m2".into(),
                    role: "user".into(),
                    text: "no match here".into(),
                    reasoning_text: String::new(),
                    image_urls: vec![],
                    state: None,
                    is_tool: false,
                    tool_call_id: None,
                    tool_name: None,
                    created_at: 0,
                }],
            ),
        ];
        let hits = collect_message_search_hits(&sessions, "beta", 10, Locale::ZhHans, true, None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "s1");
        assert_eq!(hits[0].message_id, "m1");
        assert!(hits[0].snippet.to_lowercase().contains("beta"));
    }

    #[test]
    fn message_hits_merge_stream_overlay_for_loading_assistant() {
        use crate::storage::StoredMessageState;

        let sessions = vec![sess(
            "s1",
            "Stream",
            vec![StoredMessage {
                id: "m1".into(),
                role: "assistant".into(),
                text: String::new(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            }],
        )];
        let overlay = StreamTextOverlay {
            session_id: "s1".into(),
            message_id: "m1".into(),
            answer: "partial streamed token".into(),
            reasoning: String::new(),
        };
        let hits = collect_message_search_hits(
            &sessions,
            "streamed",
            10,
            Locale::ZhHans,
            true,
            Some(&overlay),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id, "m1");

        let no_overlay =
            collect_message_search_hits(&sessions, "streamed", 10, Locale::ZhHans, true, None);
        assert!(
            no_overlay.is_empty(),
            "without overlay, stored message has no searchable text yet"
        );
    }
}
