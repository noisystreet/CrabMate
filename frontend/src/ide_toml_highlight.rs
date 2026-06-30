//! 轻量 TOML 语法高亮（HTML span + CSS），供 IDE 编辑器镜像层使用。

use crate::ide_highlight_common::push_span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Normal,
    LineComment,
    BasicString,
    MultiBasicString,
    LiteralString,
    MultiLiteralString,
}

const BOOLS: &[&str] = &["true", "false"];

struct TomlScan<'a> {
    chars: &'a [char],
    out: &'a mut String,
    buf: &'a mut String,
}

impl<'a> TomlScan<'a> {
    fn flush(&mut self, class: &str) {
        if self.buf.is_empty() {
            return;
        }
        push_span(self.out, class, self.buf);
        self.buf.clear();
    }

    fn push_token(&mut self, class: &str, text: &str) {
        push_span(self.out, class, text);
    }
}

#[must_use]
pub fn highlight_toml_to_html(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0usize;
    let mut state = State::Normal;
    let mut buf = String::new();

    while i < chars.len() {
        let mut scan = TomlScan {
            chars: &chars,
            out: &mut out,
            buf: &mut buf,
        };
        i = match state {
            State::LineComment => toml_step_line_comment(&mut scan, i, &mut state),
            State::BasicString => toml_step_basic_string(&mut scan, i, &mut state),
            State::MultiBasicString => toml_step_multi_basic_string(&mut scan, i, &mut state),
            State::LiteralString => toml_step_literal_string(&mut scan, i, &mut state),
            State::MultiLiteralString => toml_step_multi_literal_string(&mut scan, i, &mut state),
            State::Normal => toml_step_normal(&mut scan, i, &mut state),
        };
    }

    let mut scan = TomlScan {
        chars: &chars,
        out: &mut out,
        buf: &mut buf,
    };
    match state {
        State::LineComment => scan.flush("hl-com"),
        State::BasicString
        | State::MultiBasicString
        | State::LiteralString
        | State::MultiLiteralString => scan.flush("hl-str"),
        State::Normal => scan.flush("hl-plain"),
    }

    out
}

fn toml_step_line_comment(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\n' {
        scan.flush("hl-com");
        *state = State::Normal;
    }
    i + 1
}

fn toml_step_basic_string(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\\' && i + 1 < scan.chars.len() {
        scan.buf.push(scan.chars[i + 1]);
        return i + 2;
    }
    if c == '"' {
        scan.flush("hl-str");
        *state = State::Normal;
    }
    i + 1
}

fn toml_step_multi_basic_string(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '"' && triple_quote_at(scan.chars, i) {
        scan.buf.push(scan.chars[i + 1]);
        scan.buf.push(scan.chars[i + 2]);
        scan.flush("hl-str");
        *state = State::Normal;
        return i + 3;
    }
    i + 1
}

fn toml_step_literal_string(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\'' {
        scan.flush("hl-str");
        *state = State::Normal;
    }
    i + 1
}

fn toml_step_multi_literal_string(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\'' && triple_single_at(scan.chars, i) {
        scan.buf.push(scan.chars[i + 1]);
        scan.buf.push(scan.chars[i + 2]);
        scan.flush("hl-str");
        *state = State::Normal;
        return i + 3;
    }
    i + 1
}

fn toml_step_normal(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    if c == '#' {
        return toml_normal_hash(scan, i, state);
    }
    if triple_quote_at(scan.chars, i) {
        return toml_normal_triple_double(scan, i, state);
    }
    if c == '"' {
        return toml_normal_double_quote(scan, i, state);
    }
    if triple_single_at(scan.chars, i) {
        return toml_normal_triple_single(scan, i, state);
    }
    if c == '\'' {
        return toml_normal_single_quote(scan, i, state);
    }
    if c == '[' {
        return toml_normal_table(scan, i);
    }
    if c.is_ascii_digit()
        || (c == '-' && i + 1 < scan.chars.len() && scan.chars[i + 1].is_ascii_digit())
    {
        // 先尝试 RFC3339/TOML 日期时间字面量；匹配失败再退回普通数字。
        if c.is_ascii_digit() {
            if let Some(end) = toml_datetime_end(scan.chars, i) {
                scan.flush("hl-plain");
                let piece: String = scan.chars[i..end].iter().collect();
                scan.push_token("hl-num", &piece);
                return end;
            }
        }
        return toml_normal_number(scan, i);
    }
    if is_toml_key_start(c) {
        return toml_normal_key(scan, i);
    }
    scan.buf.push(c);
    i + 1
}

fn toml_normal_hash(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    scan.flush("hl-plain");
    scan.buf.push('#');
    *state = State::LineComment;
    i + 1
}

fn toml_normal_triple_double(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    scan.flush("hl-plain");
    for _ in 0..3 {
        scan.buf.push('"');
    }
    *state = State::MultiBasicString;
    i + 3
}

fn toml_normal_double_quote(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    scan.flush("hl-plain");
    scan.buf.push('"');
    *state = State::BasicString;
    i + 1
}

fn toml_normal_triple_single(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    scan.flush("hl-plain");
    for _ in 0..3 {
        scan.buf.push('\'');
    }
    *state = State::MultiLiteralString;
    i + 3
}

fn toml_normal_single_quote(scan: &mut TomlScan<'_>, i: usize, state: &mut State) -> usize {
    scan.flush("hl-plain");
    scan.buf.push('\'');
    *state = State::LiteralString;
    i + 1
}

fn toml_normal_table(scan: &mut TomlScan<'_>, i: usize) -> usize {
    scan.flush("hl-plain");
    let end = bracket_end(scan.chars, i, '[', ']');
    let piece: String = scan.chars[i..end].iter().collect();
    scan.push_token("hl-attr", &piece);
    end
}

fn toml_normal_number(scan: &mut TomlScan<'_>, i: usize) -> usize {
    scan.flush("hl-plain");
    let end = toml_number_end(scan.chars, i);
    let piece: String = scan.chars[i..end].iter().collect();
    scan.push_token("hl-num", &piece);
    end
}

fn toml_normal_key(scan: &mut TomlScan<'_>, i: usize) -> usize {
    scan.flush("hl-plain");
    let end = toml_key_end(scan.chars, i);
    let piece: String = scan.chars[i..end].iter().collect();
    let class = if is_bool(&piece) {
        "hl-kw"
    } else if is_key_at(scan.chars, end) {
        "hl-key"
    } else if is_inf_nan(&piece) {
        "hl-num"
    } else {
        "hl-plain"
    };
    scan.push_token(class, &piece);
    end
}

fn triple_quote_at(chars: &[char], i: usize) -> bool {
    i + 2 < chars.len() && chars[i] == '"' && chars[i + 1] == '"' && chars[i + 2] == '"'
}

fn triple_single_at(chars: &[char], i: usize) -> bool {
    i + 2 < chars.len() && chars[i] == '\'' && chars[i + 1] == '\'' && chars[i + 2] == '\''
}

fn bracket_end(chars: &[char], start: usize, open: char, close: char) -> usize {
    let mut depth = 0i32;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    chars.len()
}

fn is_toml_key_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn toml_key_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len()
        && (chars[i] == '_' || chars[i] == '-' || chars[i].is_ascii_alphanumeric())
    {
        i += 1;
    }
    i
}

fn skip_ws(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && matches!(chars[i], ' ' | '\t') {
        i += 1;
    }
    i
}

fn is_key_at(chars: &[char], i: usize) -> bool {
    let j = skip_ws(chars, i);
    j < chars.len() && chars[j] == '='
}

fn is_bool(s: &str) -> bool {
    BOOLS.contains(&s)
}

fn is_inf_nan(s: &str) -> bool {
    matches!(s, "inf" | "nan")
}

/// 判断 `chars[i..]` 起始的 `n` 位是否均为 ASCII 数字；匹配则返回结束下标。
fn match_n_digits(chars: &[char], i: usize, n: usize) -> Option<usize> {
    if i + n <= chars.len() && chars[i..i + n].iter().all(|c| c.is_ascii_digit()) {
        Some(i + n)
    } else {
        None
    }
}

/// 尝试匹配 `YYYY-MM-DD`，返回结束下标。不校验日期合法性（TOML 只看格式）。
fn try_date(chars: &[char], i: usize) -> Option<usize> {
    let y = match_n_digits(chars, i, 4)?;
    if chars.get(y) != Some(&'-') {
        return None;
    }
    let m = match_n_digits(chars, y + 1, 2)?;
    if chars.get(m) != Some(&'-') {
        return None;
    }
    match_n_digits(chars, m + 1, 2)
}

/// 尝试匹配 `HH:MM:SS` 可选 `.ffffff`，返回结束下标。
fn try_time(chars: &[char], i: usize) -> Option<usize> {
    let h = match_n_digits(chars, i, 2)?;
    if chars.get(h) != Some(&':') {
        return None;
    }
    let mi = match_n_digits(chars, h + 1, 2)?;
    if chars.get(mi) != Some(&':') {
        return None;
    }
    let s = match_n_digits(chars, mi + 1, 2)?;
    let mut end = s;
    if chars.get(end) == Some(&'.') {
        let mut j = end + 1;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j > end + 1 {
            end = j;
        }
    }
    Some(end)
}

/// 尝试匹配时区 `Z`/`z` 或 `(+|-)HH:MM`，返回结束下标。
fn try_tz(chars: &[char], i: usize) -> Option<usize> {
    if matches!(chars.get(i), Some(&'Z') | Some(&'z')) {
        return Some(i + 1);
    }
    let sign = chars.get(i)?;
    if *sign != '+' && *sign != '-' {
        return None;
    }
    let h = match_n_digits(chars, i + 1, 2)?;
    if chars.get(h) != Some(&':') {
        return None;
    }
    match_n_digits(chars, h + 1, 2)
}

/// 尝试匹配 TOML 日期时间字面量（date / time / datetime-with-tz）。
/// 匹配成功返回结束下标；普通整数（如 `123`）返回 `None` 以便退回普通数字路径。
fn toml_datetime_end(chars: &[char], start: usize) -> Option<usize> {
    // 仅 time
    if let Some(after_time) = try_time(chars, start) {
        let mut end = after_time;
        if let Some(after_tz) = try_tz(chars, end) {
            end = after_tz;
        }
        return Some(end);
    }
    // date [+ [T|t|space] time [+ tz]]
    let after_date = try_date(chars, start)?;
    let mut end = after_date;
    // date 后可选 `T`/`t`/空格 + time
    if end < chars.len() && (chars[end] == 'T' || chars[end] == 't' || chars[end] == ' ') {
        if let Some(after_time) = try_time(chars, end + 1) {
            end = after_time;
            if let Some(after_tz) = try_tz(chars, end) {
                end = after_tz;
            }
            return Some(end);
        }
        // 空格后不是 time → date 单独成立，不消费空格
        if chars[end] == ' ' {
            return Some(end);
        }
        // `T`/`t` 后无 time → date 仍成立（消费 T 当作分隔不合理，保持 end = after_date）
    }
    // date 后可选直接跟时区（罕见）
    if let Some(after_tz) = try_tz(chars, end) {
        end = after_tz;
    }
    Some(end)
}

fn toml_number_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    if i < chars.len() && chars[i] == '-' {
        i += 1;
    }
    if i + 1 < chars.len() && chars[i] == '0' {
        let next = chars[i + 1];
        if next == 'x' || next == 'o' || next == 'b' {
            i += 2;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            return i;
        }
    }
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
        i += 1;
    }
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
            i += 1;
        }
    }
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let t = i;
        i += 1;
        if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
            i += 1;
        }
        let mut any = false;
        while i < chars.len() && chars[i].is_ascii_digit() {
            any = true;
            i += 1;
        }
        if any {
            return i;
        }
        i = t;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_toml_keys_and_comments() {
        let html = highlight_toml_to_html("# c\nkey = \"v\"\n");
        assert!(html.contains("hl-com"));
        assert!(html.contains("hl-key"));
        assert!(html.contains("hl-str"));
    }

    #[test]
    fn highlights_table_header() {
        let html = highlight_toml_to_html("[package]\n");
        assert!(html.contains("hl-attr"));
        assert!(html.contains("[package]"));
    }

    #[test]
    fn highlights_inf_and_nan() {
        let html = highlight_toml_to_html("a = inf\nb = nan\n");
        assert!(html.contains("hl-num"));
        assert!(html.contains("inf"));
        assert!(html.contains("nan"));
        // 普通整数仍是 num，bool 仍是 kw
        let html2 = highlight_toml_to_html("x = 42\ny = true\n");
        assert!(html2.contains("hl-num"));
        assert!(html2.contains("hl-kw"));
    }

    #[test]
    fn highlights_datetime_literals() {
        let html = highlight_toml_to_html("created = 2026-01-01\n");
        assert!(html.contains("hl-num"));
        assert!(html.contains("2026-01-01"));
        // 带时区的完整 RFC3339
        let html2 = highlight_toml_to_html("t = 2026-01-01T07:32:00Z\n");
        assert!(html2.contains("hl-num"));
        assert!(html2.contains("2026-01-01T07:32:00Z"));
        // 仅时间
        let html3 = highlight_toml_to_html("t = 07:32:00\n");
        assert!(html3.contains("hl-num"));
        assert!(html3.contains("07:32:00"));
        // 普通整数不应被误判为日期
        let html4 = highlight_toml_to_html("n = 12345\n");
        assert!(html4.contains("hl-num"));
        assert!(html4.contains("12345"));
    }
}
