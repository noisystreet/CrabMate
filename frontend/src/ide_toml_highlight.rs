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
}
