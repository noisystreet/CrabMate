//! IDE 编辑器内查找与跳转行（`textarea` 选区）。

use leptos::prelude::Get;
use wasm_bindgen::JsCast;

/// 在文本中查找不区分大小写的匹配，返回 `(start_char, end_char)` 列表。
#[must_use]
pub fn find_match_ranges(text: &str, query: &str) -> Vec<(usize, usize)> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let lower_text: String = text.to_lowercase();
    let lower_q: String = q.to_lowercase();
    let q_len = lower_q.chars().count();
    if q_len == 0 {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut search_from = 0usize;
    while search_from < lower_text.len() {
        let Some(rel) = lower_text[search_from..].find(&lower_q) else {
            break;
        };
        let byte_start = search_from + rel;
        let byte_end = byte_start + lower_q.len();
        let start_char = text[..byte_start].chars().count();
        let end_char = start_char + q_len;
        ranges.push((start_char, end_char));
        search_from = byte_end;
    }
    ranges
}

/// 将 `char` 下标转为 UTF-16 偏移（`textarea` selection API 使用 UTF-16 code units）。
fn char_index_to_utf16(text: &str, char_idx: usize) -> u32 {
    text.chars()
        .take(char_idx)
        .map(|c| c.len_utf16())
        .sum::<usize>() as u32
}

/// 在 `textarea` 上设置选区并滚动到可见。
pub fn apply_textarea_selection(
    ta: &web_sys::HtmlTextAreaElement,
    start_char: usize,
    end_char: usize,
) {
    let text = ta.value();
    let start = char_index_to_utf16(&text, start_char);
    let end = char_index_to_utf16(&text, end_char);
    let _ = ta.focus();
    let _ = ta.set_selection_start(Some(start));
    let _ = ta.set_selection_end(Some(end));
}

/// 跳转到指定行（1-based）；行号越界时钳制到末行。
pub fn goto_line_in_textarea(ta: &web_sys::HtmlTextAreaElement, line_one_based: usize) {
    let text = ta.value();
    let total_lines = text.lines().count().max(1);
    let line = line_one_based.clamp(1, total_lines);
    let mut char_offset = 0usize;
    for (i, ln) in text.lines().enumerate() {
        if i + 1 == line {
            break;
        }
        char_offset += ln.chars().count() + 1;
    }
    apply_textarea_selection(ta, char_offset, char_offset);
}

/// 从 `NodeRef` 解析 `HtmlTextAreaElement`。
#[must_use]
pub fn textarea_from_ref(
    textarea_ref: &leptos::prelude::NodeRef<leptos::html::Textarea>,
) -> Option<web_sys::HtmlTextAreaElement> {
    textarea_ref
        .get()
        .and_then(|n| n.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_case_insensitive_matches() {
        let ranges = find_match_ranges("Foo bar foo", "foo");
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], (0, 3));
        assert_eq!(ranges[1], (8, 11));
    }

    #[test]
    fn empty_query_returns_no_matches() {
        assert!(find_match_ranges("hello", "").is_empty());
        assert!(find_match_ranges("hello", "   ").is_empty());
    }
}
