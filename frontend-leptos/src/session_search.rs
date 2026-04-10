//! 侧栏会话过滤与跨会话消息全文搜索（本地内存扫描，不建持久索引）。

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

use std::mem;

use crate::i18n::Locale;
use crate::message_format::message_text_for_display;
use crate::storage::ChatSession;

/// 规范化查询：小写、折叠空白。
pub fn normalize_search_query(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// 会话内查找：将展示文本按命中切片，用于 `<mark>` 内联高亮。
///
/// `needle_lower` 须为 [`normalize_search_query`] 的结果。按 Unicode 标量值窗口做大小写不敏感比较；
/// 命中区间合并（重叠子串并集）。
pub fn split_for_find_highlight(haystack: &str, needle_lower: &str) -> Vec<(String, bool)> {
    if needle_lower.is_empty() {
        return vec![(haystack.to_string(), false)];
    }
    let hay_chars: Vec<char> = haystack.chars().collect();
    let n = needle_lower.chars().count();
    if n == 0 || hay_chars.len() < n {
        return vec![(haystack.to_string(), false)];
    }
    let mut marked = vec![false; hay_chars.len()];
    for start in 0..=hay_chars.len() - n {
        let slice: String = hay_chars[start..start + n].iter().collect();
        if slice.to_lowercase() == needle_lower {
            for m in &mut marked[start..start + n] {
                *m = true;
            }
        }
    }
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut cur = String::new();
    let mut cur_hl: Option<bool> = None;
    for (i, &ch) in hay_chars.iter().enumerate() {
        let hl = marked[i];
        match cur_hl {
            None => {
                cur_hl = Some(hl);
                cur.push(ch);
            }
            Some(prev) if prev == hl => cur.push(ch),
            Some(prev) => {
                out.push((mem::take(&mut cur), prev));
                cur.push(ch);
                cur_hl = Some(hl);
            }
        }
    }
    if !cur.is_empty() {
        if let Some(hl) = cur_hl {
            out.push((cur, hl));
        }
    }
    if out.is_empty() {
        vec![(haystack.to_string(), false)]
    } else {
        out
    }
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
pub fn collect_message_search_hits(
    sessions: &[ChatSession],
    needle_lower: &str,
    max_hits: usize,
    loc: Locale,
) -> Vec<MessageSearchHit> {
    if needle_lower.is_empty() || max_hits == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for s in sessions {
        for m in &s.messages {
            let display = message_text_for_display(m, loc);
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

/// 将 `id="msg-{msg_id}"` 的气泡滚入主消息区可视范围（仅 WASM）。
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
    let eid = format!("msg-{msg_id}");
    let Some(el) = doc.get_element_by_id(&eid) else {
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
            title: title.to_string(),
            draft: String::new(),
            messages,
            updated_at: 0,
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
    fn find_highlight_splits_ascii_case_insensitive() {
        let segs = split_for_find_highlight("Hello HELLO hello", "hello");
        let joined: String = segs.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, "Hello HELLO hello");
        assert!(segs.iter().any(|(_, hl)| *hl));
    }

    #[test]
    fn find_highlight_cjk_unchanged_length() {
        let segs = split_for_find_highlight("你好世界你好", "你好");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].0, "你好");
        assert!(segs[0].1);
        assert_eq!(segs[1].0, "世界");
        assert!(!segs[1].1);
        assert_eq!(segs[2].0, "你好");
        assert!(segs[2].1);
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
                    image_urls: vec![],
                    state: None,
                    is_tool: false,
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
                    image_urls: vec![],
                    state: None,
                    is_tool: false,
                    created_at: 0,
                }],
            ),
        ];
        let hits = collect_message_search_hits(&sessions, "beta", 10, Locale::ZhHans);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "s1");
        assert_eq!(hits[0].message_id, "m1");
        assert!(hits[0].snippet.to_lowercase().contains("beta"));
    }
}
