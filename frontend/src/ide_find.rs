//! IDE 编辑器内查找与跳转行。

use crate::ide_codemirror::IdeEditorHost;

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

/// 在编辑器上设置选区并滚动到可见。
pub fn apply_editor_selection(host: &IdeEditorHost, start_char: usize, end_char: usize) {
    host.set_selection_chars(start_char, end_char);
}

/// 跳转到指定行（1-based）；行号越界时钳制到末行。
pub fn goto_line_in_editor(host: &IdeEditorHost, line_one_based: usize) {
    host.goto_line(line_one_based);
}
