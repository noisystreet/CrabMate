//! 轻量 JSON 语法高亮。

use crate::ide_highlight_common::push_span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Normal,
    DoubleQuote,
    Escape,
}

#[must_use]
pub fn highlight_json_to_html(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let chars: Vec<char> = source.chars().collect();
    let mut buf = String::new();
    let mut state = State::Normal;
    let mut i = 0usize;
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
            State::DoubleQuote => {
                buf.push(c);
                if c == '\\' {
                    state = State::Escape;
                } else if c == '"' {
                    flush(&mut out, "hl-str", &mut buf);
                    state = State::Normal;
                }
                i += 1;
            }
            State::Escape => {
                buf.push(c);
                state = State::DoubleQuote;
                i += 1;
            }
            State::Normal => {
                if c == '"' {
                    buf.push(c);
                    state = State::DoubleQuote;
                    i += 1;
                } else if c.is_ascii_whitespace()
                    || c == '{'
                    || c == '}'
                    || c == '['
                    || c == ']'
                    || c == ':'
                    || c == ','
                {
                    buf.push(c);
                    flush(&mut out, "hl-plain", &mut buf);
                    i += 1;
                } else if c == 't' && source[i..].starts_with("true") {
                    push_span(&mut out, "hl-kw", "true");
                    i += 4;
                } else if c == 'f' && source[i..].starts_with("false") {
                    push_span(&mut out, "hl-kw", "false");
                    i += 5;
                } else if c == 'n' && source[i..].starts_with("null") {
                    push_span(&mut out, "hl-kw", "null");
                    i += 4;
                } else if c == '-' || c.is_ascii_digit() {
                    let start = i;
                    i += 1;
                    while i < chars.len()
                        && (chars[i].is_ascii_digit()
                            || chars[i] == '.'
                            || chars[i] == 'e'
                            || chars[i] == 'E'
                            || chars[i] == '+'
                            || chars[i] == '-')
                    {
                        i += 1;
                    }
                    let num: String = chars[start..i].iter().collect();
                    push_span(&mut out, "hl-num", &num);
                } else {
                    buf.push(c);
                    flush(&mut out, "hl-plain", &mut buf);
                    i += 1;
                }
            }
        }
    }
    match state {
        State::DoubleQuote | State::Escape => flush(&mut out, "hl-str", &mut buf),
        State::Normal => flush(&mut out, "hl-plain", &mut buf),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_smoke() {
        let html = highlight_json_to_html(r#"{"ok":true,"n":42}"#);
        assert!(html.contains("hl-str"));
        assert!(html.contains("hl-kw"));
        assert!(html.contains("hl-num"));
    }
}
