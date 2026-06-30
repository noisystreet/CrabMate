//! 轻量 Shell / Bash 语法高亮。

use crate::ide_highlight_common::push_span;

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "in",
    "function", "select", "until", "return", "exit", "export", "local", "readonly", "declare",
    "set", "unset", "shift", "break", "continue", "trap", "source", "alias", "true", "false",
];

#[must_use]
pub fn highlight_shell_to_html(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let chars: Vec<char> = source.chars().collect();
    let mut buf = String::new();
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
        if c == '#' {
            let start = i;
            while i < chars.len() {
                i += 1;
                if i >= chars.len() || chars[i - 1] == '\n' {
                    break;
                }
            }
            let com: String = chars[start..i].iter().collect();
            push_span(&mut out, "hl-com", &com);
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            buf.push(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                buf.push(ch);
                i += 1;
                if ch == '\\' && i < chars.len() {
                    buf.push(chars[i]);
                    i += 1;
                    continue;
                }
                if ch == quote {
                    break;
                }
            }
            flush(&mut out, "hl-str", &mut buf);
            continue;
        }
        if c == '$' {
            flush(&mut out, "hl-plain", &mut buf);
            let start = i;
            i += 1;
            if i < chars.len() && chars[i] == '{' {
                buf.push('$');
                buf.push('{');
                i += 1;
                while i < chars.len() {
                    buf.push(chars[i]);
                    i += 1;
                    if chars[i - 1] == '}' {
                        break;
                    }
                }
            } else {
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '?')
                {
                    buf.push(chars[i]);
                    i += 1;
                }
            }
            let var: String = chars[start..i].iter().collect();
            push_span(&mut out, "hl-type", &var);
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if KEYWORDS.contains(&word.as_str()) {
                push_span(&mut out, "hl-kw", &word);
            } else {
                push_span(&mut out, "hl-plain", &word);
            }
            continue;
        }
        buf.push(c);
        flush(&mut out, "hl-plain", &mut buf);
        i += 1;
    }
    flush(&mut out, "hl-plain", &mut buf);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_smoke() {
        assert!(highlight_shell_to_html("# comment\nif true; then\nfi\n").contains("hl-com"));
        assert!(highlight_shell_to_html("export PATH=1").contains("hl-kw"));
    }
}
