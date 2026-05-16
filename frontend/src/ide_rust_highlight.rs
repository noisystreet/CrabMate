//! 轻量 Rust 语法高亮（HTML span + CSS），供 IDE 编辑器镜像层使用。

use crate::ide_highlight_common::push_span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Normal,
    LineComment,
    BlockComment,
    DoubleQuote,
    CharLiteral,
    RawString { hashes: usize },
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

const BUILTIN_TYPES: &[&str] = &[
    "bool", "char", "str", "String", "Option", "Result", "Vec", "Box", "Rc", "Arc", "usize",
    "isize", "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "i128", "f32", "f64",
];

#[must_use]
pub fn highlight_rust_to_html(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0usize;
    let mut state = State::Normal;
    let mut buf = String::new();

    let flush = |out: &mut String, class: &str, buf: &mut String| {
        if buf.is_empty() {
            return;
        }
        push_span(out, class, buf);
        buf.clear();
    };

    while i < chars.len() {
        let c = chars[i];
        match state {
            State::LineComment => {
                buf.push(c);
                if c == '\n' {
                    flush(&mut out, "hl-com", &mut buf);
                    state = State::Normal;
                }
                i += 1;
            }
            State::BlockComment => {
                buf.push(c);
                if c == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                    buf.push(chars[i + 1]);
                    i += 2;
                    flush(&mut out, "hl-com", &mut buf);
                    state = State::Normal;
                } else {
                    i += 1;
                }
            }
            State::DoubleQuote => {
                buf.push(c);
                if c == '\\' && i + 1 < chars.len() {
                    buf.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if c == '"' {
                    flush(&mut out, "hl-str", &mut buf);
                    state = State::Normal;
                }
                i += 1;
            }
            State::CharLiteral => {
                buf.push(c);
                if c == '\\' && i + 1 < chars.len() {
                    buf.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if c == '\'' {
                    flush(&mut out, "hl-str", &mut buf);
                    state = State::Normal;
                }
                i += 1;
            }
            State::RawString { hashes } => {
                buf.push(c);
                if c == '"' {
                    let mut close_hashes = 0usize;
                    let mut j = i + 1;
                    while j < chars.len() && chars[j] == '#' {
                        close_hashes += 1;
                        buf.push(chars[j]);
                        j += 1;
                    }
                    if close_hashes == hashes {
                        i = j;
                        flush(&mut out, "hl-str", &mut buf);
                        state = State::Normal;
                        continue;
                    }
                }
                i += 1;
            }
            State::Normal => {
                if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
                    flush(&mut out, "hl-plain", &mut buf);
                    buf.push('/');
                    buf.push('/');
                    i += 2;
                    state = State::LineComment;
                    continue;
                }
                if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
                    flush(&mut out, "hl-plain", &mut buf);
                    buf.push('/');
                    buf.push('*');
                    i += 2;
                    state = State::BlockComment;
                    continue;
                }
                if c == '#' && i + 1 < chars.len() && (chars[i + 1] == '[' || chars[i + 1] == '!') {
                    flush(&mut out, "hl-plain", &mut buf);
                    let end = attribute_end(&chars, i);
                    let piece: String = chars[i..end].iter().collect();
                    push_span(&mut out, "hl-attr", &piece);
                    i = end;
                    continue;
                }
                if c == 'b' && i + 1 < chars.len() && chars[i + 1] == '"' {
                    flush(&mut out, "hl-plain", &mut buf);
                    buf.push('b');
                    buf.push('"');
                    i += 2;
                    state = State::DoubleQuote;
                    continue;
                }
                if c == 'r' && i + 1 < chars.len() && (chars[i + 1] == '"' || chars[i + 1] == '#') {
                    flush(&mut out, "hl-plain", &mut buf);
                    let (hashes, next_i) = parse_raw_string_hashes(&chars, i);
                    buf.push('r');
                    for _ in 0..hashes {
                        buf.push('#');
                    }
                    if next_i < chars.len() {
                        buf.push(chars[next_i]);
                    }
                    i = next_i + 1;
                    state = State::RawString { hashes };
                    continue;
                }
                if c == '"' {
                    flush(&mut out, "hl-plain", &mut buf);
                    buf.push('"');
                    i += 1;
                    state = State::DoubleQuote;
                    continue;
                }
                if c == '\''
                    && i + 1 < chars.len()
                    && (chars[i + 1].is_ascii_alphabetic() || chars[i + 1] == '_')
                {
                    flush(&mut out, "hl-plain", &mut buf);
                    buf.push('\'');
                    i += 1;
                    state = State::CharLiteral;
                    continue;
                }
                if c.is_ascii_digit()
                    || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
                {
                    flush(&mut out, "hl-plain", &mut buf);
                    let end = number_end(&chars, i);
                    let piece: String = chars[i..end].iter().collect();
                    push_span(&mut out, "hl-num", &piece);
                    i = end;
                    continue;
                }
                if is_ident_start(c) {
                    flush(&mut out, "hl-plain", &mut buf);
                    let end = ident_end(&chars, i);
                    let piece: String = chars[i..end].iter().collect();
                    let class = if KEYWORDS.contains(&piece.as_str()) {
                        "hl-kw"
                    } else if BUILTIN_TYPES.contains(&piece.as_str())
                        || piece.chars().next().is_some_and(|c| c.is_uppercase())
                    {
                        "hl-type"
                    } else {
                        "hl-plain"
                    };
                    push_span(&mut out, class, &piece);
                    if end < chars.len() && chars[end] == '!' {
                        push_span(&mut out, "hl-mac", "!");
                        i = end + 1;
                    } else {
                        i = end;
                    }
                    continue;
                }
                buf.push(c);
                i += 1;
            }
        }
    }

    match state {
        State::LineComment | State::BlockComment => flush(&mut out, "hl-com", &mut buf),
        State::DoubleQuote | State::CharLiteral | State::RawString { .. } => {
            flush(&mut out, "hl-str", &mut buf)
        }
        State::Normal => flush(&mut out, "hl-plain", &mut buf),
    }

    out
}

fn parse_raw_string_hashes(chars: &[char], start: usize) -> (usize, usize) {
    let mut i = start + 1;
    let mut hashes = 0usize;
    while i < chars.len() && chars[i] == '#' {
        hashes += 1;
        i += 1;
    }
    let quote_i = i;
    (hashes, quote_i)
}

fn attribute_end(chars: &[char], start: usize) -> usize {
    let mut depth = 0i32;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '[' => depth += 1,
            ']' => {
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

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn ident_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len() && (chars[i] == '_' || chars[i].is_ascii_alphanumeric()) {
        i += 1;
    }
    i
}

fn number_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    if i < chars.len() && chars[i] == '.' {
        i += 1;
    }
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
        i += 1;
    }
    if i < chars.len() && (chars[i] == 'u' || chars[i] == 'i' || chars[i] == 'f') {
        let t = i;
        i += 1;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
            i += 1;
        }
        if i > t + 1 {
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
    fn highlights_keywords_and_comments() {
        let html = highlight_rust_to_html("fn main() { // hi\n}");
        assert!(html.contains("hl-kw"));
        assert!(html.contains("fn"));
        assert!(html.contains("hl-com"));
        assert!(html.contains("// hi"));
    }

    #[test]
    fn highlights_strings() {
        let html = highlight_rust_to_html(r#"let s = "a\"b";"#);
        assert!(html.contains("hl-str"));
    }

    #[test]
    fn escapes_html_in_source() {
        let html = highlight_rust_to_html("<script>");
        assert!(html.contains("&lt;"));
        assert!(html.contains("&gt;"));
        assert!(!html.contains("<script>"));
    }
}
