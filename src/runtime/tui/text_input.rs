//! 多行输入：UTF-8 安全光标、按显示宽度折行（与 `Paragraph` + `Wrap` 近似）。

use unicode_width::UnicodeWidthChar;

/// 将光标钳在合法 UTF-8 字符边界上。
pub(super) fn snap_cursor_to_char_boundary(s: &str, cursor: usize) -> usize {
    if s.is_empty() {
        return 0;
    }
    let mut c = cursor.min(s.len());
    while c < s.len() && !s.is_char_boundary(c) {
        c += 1;
    }
    while c > 0 && !s.is_char_boundary(c) {
        c -= 1;
    }
    c
}

pub(super) fn insert_at_cursor(s: &mut String, cursor: &mut usize, ch: char) {
    *cursor = snap_cursor_to_char_boundary(s, *cursor);
    s.insert(*cursor, ch);
    *cursor += ch.len_utf8();
}

pub(super) fn delete_before_cursor(s: &mut String, cursor: &mut usize) {
    *cursor = snap_cursor_to_char_boundary(s, *cursor);
    if *cursor == 0 {
        return;
    }
    let prev = s[..*cursor]
        .chars()
        .next_back()
        .expect("cursor > 0 on char boundary");
    let start = *cursor - prev.len_utf8();
    s.replace_range(start..*cursor, "");
    *cursor = start;
}

pub(super) fn delete_after_cursor(s: &mut String, cursor: &mut usize) {
    *cursor = snap_cursor_to_char_boundary(s, *cursor);
    if *cursor >= s.len() {
        return;
    }
    let ch = s[*cursor..].chars().next().expect("char boundary");
    let len = ch.len_utf8();
    s.replace_range(*cursor..*cursor + len, "");
}

pub(super) fn cursor_step_left(s: &str, cursor: &mut usize) {
    *cursor = snap_cursor_to_char_boundary(s, *cursor);
    if *cursor == 0 {
        return;
    }
    let ch = s[..*cursor]
        .chars()
        .next_back()
        .expect("cursor > 0 on char boundary");
    *cursor -= ch.len_utf8();
}

pub(super) fn cursor_step_right(s: &str, cursor: &mut usize) {
    *cursor = snap_cursor_to_char_boundary(s, *cursor);
    if *cursor >= s.len() {
        return;
    }
    let ch = s[*cursor..].chars().next().expect("char boundary");
    *cursor += ch.len_utf8();
}

/// 左侧主栏（约 65% 宽）内，与 `draw_chat` 中输入 `Paragraph` 内层文本宽度一致。
pub(super) fn left_column_inner_text_width(term_cols: u16) -> usize {
    let col_w = (term_cols as u32 * 65 / 100) as u16;
    col_w.saturating_sub(2).max(1) as usize
}

/// 光标前的字符在「按 `max_width` 折行」布局下的 (行, 列宽)。
pub(super) fn coords_before_cursor(s: &str, cursor_byte: usize, max_width: usize) -> (u16, usize) {
    let cursor_byte = snap_cursor_to_char_boundary(s, cursor_byte.min(s.len()));
    let mw = max_width.max(1);
    let mut row = 0u16;
    let mut col = 0usize;
    for (idx, ch) in s.char_indices() {
        if idx >= cursor_byte {
            break;
        }
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if ch == '\n' {
            row = row.saturating_add(1);
            col = 0;
            continue;
        }
        if col + w > mw && col > 0 {
            row = row.saturating_add(1);
            col = 0;
        }
        if col + w > mw && col == 0 {
            row = row.saturating_add(1);
            col = 0;
        }
        col += w;
    }
    (row, col)
}

pub(super) fn max_visual_row(s: &str, max_width: usize) -> u16 {
    coords_before_cursor(s, s.len(), max_width).0
}

/// 将光标移动到上一 / 下一显示行（含自动折行），列尽量对齐。
pub(super) fn cursor_move_vertical(s: &str, cursor: &mut usize, max_width: usize, delta_row: i32) {
    let mw = max_width.max(1);
    let (r, c) = coords_before_cursor(s, *cursor, mw);
    let max_r = max_visual_row(s, mw);
    let nr = (r as i32 + delta_row).clamp(0, max_r as i32) as u16;
    if nr == r {
        return;
    }
    *cursor = byte_index_from_display_coords(s, mw, nr, c);
}

/// 将 `(行, 列宽)` 映射回 UTF-8 字节下标（插入点）。
pub(super) fn byte_index_from_display_coords(
    s: &str,
    max_width: usize,
    target_row: u16,
    target_col: usize,
) -> usize {
    let mw = max_width.max(1);
    let mut row = 0u16;
    let mut col = 0usize;
    let mut line_start_idx = 0usize;
    for (idx, ch) in s.char_indices() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if ch == '\n' {
            if row == target_row {
                return snap_cursor_to_char_boundary(s, idx);
            }
            row = row.saturating_add(1);
            col = 0;
            line_start_idx = idx + ch.len_utf8();
            continue;
        }
        if col + w > mw && col > 0 {
            if row == target_row {
                let want = target_col.min(col);
                return snap_cursor_to_char_boundary(
                    s,
                    byte_index_on_visual_line(s, line_start_idx, idx, mw, want),
                );
            }
            row = row.saturating_add(1);
            col = 0;
            line_start_idx = idx;
        }
        if col + w > mw && col == 0 {
            row = row.saturating_add(1);
            col = 0;
            line_start_idx = idx;
        }
        if row == target_row {
            if col + w > target_col {
                return snap_cursor_to_char_boundary(s, idx);
            }
            if col + w == target_col {
                return snap_cursor_to_char_boundary(s, idx + ch.len_utf8());
            }
        }
        col += w;
    }
    if row == target_row {
        return snap_cursor_to_char_boundary(s, s.len());
    }
    s.len()
}

/// 在 `[line_start, line_end)` 这一段（单行显示片段）内，找到显示宽度累计为 `target_col` 的字节下标。
fn byte_index_on_visual_line(
    s: &str,
    line_start: usize,
    line_end: usize,
    _mw: usize,
    target_col: usize,
) -> usize {
    let slice = &s[line_start..line_end];
    let mut acc = 0usize;
    for (i, ch) in slice.char_indices() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if acc + w > target_col {
            return line_start + i;
        }
        acc += w;
        if acc == target_col {
            return line_start + i + ch.len_utf8();
        }
    }
    line_end
}

/// 鼠标命中：相对输入区内层的列、行（0 基）→ 光标字节位置。
pub(super) fn byte_index_from_mouse_cell(
    s: &str,
    max_width: usize,
    cell_col: u16,
    cell_row: u16,
) -> usize {
    byte_index_from_display_coords(s, max_width, cell_row, cell_col as usize)
}

pub(super) fn home_logical_line(s: &str, cursor: &mut usize) {
    *cursor = snap_cursor_to_char_boundary(s, *cursor);
    let line_start = s[..*cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
    *cursor = line_start;
}

pub(super) fn end_logical_line(s: &str, cursor: &mut usize) {
    *cursor = snap_cursor_to_char_boundary(s, *cursor);
    match s[*cursor..].find('\n') {
        Some(pos) => *cursor += pos,
        None => *cursor = s.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_delete_roundtrip() {
        let mut s = "ab".to_string();
        let mut c = 1;
        insert_at_cursor(&mut s, &mut c, 'X');
        assert_eq!(s, "aXb");
        assert_eq!(c, 2);
        delete_before_cursor(&mut s, &mut c);
        assert_eq!(s, "ab");
        assert_eq!(c, 1);
    }

    #[test]
    fn coords_wrap_ascii() {
        let s = "abcdef";
        let mw = 3;
        assert_eq!(coords_before_cursor(s, 0, mw), (0, 0));
        assert_eq!(coords_before_cursor(s, 3, mw), (0, 3));
        assert_eq!(coords_before_cursor(s, 4, mw), (1, 1));
        assert_eq!(coords_before_cursor(s, 6, mw), (1, 3));
    }

    #[test]
    fn vertical_up_preserves_col_approx() {
        let s = "abcdef";
        let mw = 3;
        let mut cur = 5;
        cursor_move_vertical(s, &mut cur, mw, -1);
        assert_eq!(cur, 2);
    }
}
