//! 轻量 YAML 语法高亮（HTML span + CSS），供 IDE 编辑器镜像层使用。

use crate::ide_highlight_common::push_span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Normal,
    LineComment,
    DoubleQuote,
    SingleQuote,
}

const KEYWORDS: &[&str] = &[
    "true", "false", "null", "True", "False", "NULL", "yes", "no", "on", "off", "Yes", "No", "On",
    "Off",
];

struct YamlScan<'a> {
    chars: &'a [char],
    out: &'a mut String,
    buf: &'a mut String,
    line_start: &'a mut bool,
}

impl<'a> YamlScan<'a> {
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
pub fn highlight_yaml_to_html(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0usize;
    let mut state = State::Normal;
    let mut buf = String::new();
    let mut line_start = true;

    while i < chars.len() {
        let mut scan = YamlScan {
            chars: &chars,
            out: &mut out,
            buf: &mut buf,
            line_start: &mut line_start,
        };
        i = match state {
            State::LineComment => yaml_step_line_comment(&mut scan, i, &mut state),
            State::DoubleQuote => yaml_step_double_quote(&mut scan, i, &mut state),
            State::SingleQuote => yaml_step_single_quote(&mut scan, i, &mut state),
            State::Normal => yaml_step_normal(&mut scan, i, &mut state),
        };
    }

    let mut scan = YamlScan {
        chars: &chars,
        out: &mut out,
        buf: &mut buf,
        line_start: &mut line_start,
    };
    match state {
        State::LineComment => scan.flush("hl-com"),
        State::DoubleQuote | State::SingleQuote => scan.flush("hl-str"),
        State::Normal => scan.flush("hl-plain"),
    }

    out
}

fn yaml_step_line_comment(scan: &mut YamlScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\n' {
        scan.flush("hl-com");
        *state = State::Normal;
        *scan.line_start = true;
    }
    i + 1
}

fn yaml_step_double_quote(scan: &mut YamlScan<'_>, i: usize, state: &mut State) -> usize {
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

fn yaml_step_single_quote(scan: &mut YamlScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\'' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '\'' {
        scan.buf.push('\'');
        return i + 2;
    }
    if c == '\'' {
        scan.flush("hl-str");
        *state = State::Normal;
    }
    i + 1
}

fn yaml_step_normal(scan: &mut YamlScan<'_>, i: usize, state: &mut State) -> usize {
    if let Some(next) = yaml_try_normal_newline(scan, i) {
        return next;
    }
    if let Some(next) = yaml_try_normal_comment(scan, i, state) {
        return next;
    }
    if let Some(next) = yaml_try_normal_doc_marker(scan, i) {
        return next;
    }
    if let Some(next) = yaml_try_normal_double_quote(scan, i, state) {
        return next;
    }
    if let Some(next) = yaml_try_normal_single_quote(scan, i, state) {
        return next;
    }
    if let Some(next) = yaml_try_normal_anchor(scan, i) {
        return next;
    }
    if let Some(next) = yaml_try_normal_tag(scan, i) {
        return next;
    }
    if let Some(next) = yaml_try_normal_number(scan, i) {
        return next;
    }
    if let Some(next) = yaml_try_normal_key(scan, i) {
        return next;
    }
    yaml_normal_plain_char(scan, i)
}

fn yaml_try_normal_newline(scan: &mut YamlScan<'_>, i: usize) -> Option<usize> {
    (scan.chars[i] == '\n').then(|| yaml_normal_newline(scan, i))
}

fn yaml_try_normal_comment(scan: &mut YamlScan<'_>, i: usize, state: &mut State) -> Option<usize> {
    let c = scan.chars[i];
    (c == '#' && (*scan.line_start || is_yaml_comment_hash(scan.chars, i)))
        .then(|| yaml_normal_comment(scan, i, state))
}

fn yaml_try_normal_doc_marker(scan: &mut YamlScan<'_>, i: usize) -> Option<usize> {
    let c = scan.chars[i];
    (*scan.line_start && c == '-' && yaml_doc_marker_at(scan.chars, i))
        .then(|| yaml_normal_doc_marker(scan, i))
}

fn yaml_try_normal_double_quote(
    scan: &mut YamlScan<'_>,
    i: usize,
    state: &mut State,
) -> Option<usize> {
    (scan.chars[i] == '"').then(|| yaml_normal_double_quote(scan, i, state))
}

fn yaml_try_normal_single_quote(
    scan: &mut YamlScan<'_>,
    i: usize,
    state: &mut State,
) -> Option<usize> {
    (scan.chars[i] == '\'').then(|| yaml_normal_single_quote(scan, i, state))
}

fn yaml_try_normal_anchor(scan: &mut YamlScan<'_>, i: usize) -> Option<usize> {
    matches!(scan.chars[i], '&' | '*').then(|| yaml_normal_anchor(scan, i))
}

fn yaml_try_normal_tag(scan: &mut YamlScan<'_>, i: usize) -> Option<usize> {
    let c = scan.chars[i];
    (c == '!' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '!')
        .then(|| yaml_normal_tag(scan, i))
}

fn yaml_try_normal_number(scan: &mut YamlScan<'_>, i: usize) -> Option<usize> {
    let c = scan.chars[i];
    let is_num = c.is_ascii_digit()
        || (c == '-' && i + 1 < scan.chars.len() && scan.chars[i + 1].is_ascii_digit());
    is_num.then(|| yaml_normal_number(scan, i))
}

fn yaml_try_normal_key(scan: &mut YamlScan<'_>, i: usize) -> Option<usize> {
    is_yaml_key_start(scan.chars[i]).then(|| yaml_normal_key(scan, i))
}

fn yaml_normal_plain_char(scan: &mut YamlScan<'_>, i: usize) -> usize {
    let c = scan.chars[i];
    if !c.is_whitespace() {
        *scan.line_start = false;
    }
    scan.buf.push(c);
    i + 1
}

fn yaml_normal_newline(scan: &mut YamlScan<'_>, i: usize) -> usize {
    scan.flush("hl-plain");
    scan.out.push('\n');
    *scan.line_start = true;
    i + 1
}

fn yaml_normal_comment(scan: &mut YamlScan<'_>, i: usize, state: &mut State) -> usize {
    scan.flush("hl-plain");
    scan.buf.push('#');
    *state = State::LineComment;
    i + 1
}

fn yaml_doc_marker_at(chars: &[char], i: usize) -> bool {
    i + 2 < chars.len() && chars[i + 1] == '-' && chars[i + 2] == '-'
}

fn yaml_normal_doc_marker(scan: &mut YamlScan<'_>, i: usize) -> usize {
    scan.flush("hl-plain");
    let end = yaml_doc_marker_end(scan.chars, i);
    let piece: String = scan.chars[i..end].iter().collect();
    scan.push_token("hl-attr", &piece);
    *scan.line_start = false;
    end
}

fn yaml_normal_double_quote(scan: &mut YamlScan<'_>, i: usize, state: &mut State) -> usize {
    scan.flush("hl-plain");
    scan.buf.push('"');
    *state = State::DoubleQuote;
    *scan.line_start = false;
    i + 1
}

fn yaml_normal_single_quote(scan: &mut YamlScan<'_>, i: usize, state: &mut State) -> usize {
    scan.flush("hl-plain");
    scan.buf.push('\'');
    *state = State::SingleQuote;
    *scan.line_start = false;
    i + 1
}

fn yaml_normal_anchor(scan: &mut YamlScan<'_>, i: usize) -> usize {
    scan.flush("hl-plain");
    let end = yaml_anchor_end(scan.chars, i);
    let piece: String = scan.chars[i..end].iter().collect();
    scan.push_token("hl-mac", &piece);
    *scan.line_start = false;
    end
}

fn yaml_normal_tag(scan: &mut YamlScan<'_>, i: usize) -> usize {
    scan.flush("hl-plain");
    let end = yaml_tag_end(scan.chars, i);
    let piece: String = scan.chars[i..end].iter().collect();
    scan.push_token("hl-type", &piece);
    *scan.line_start = false;
    end
}

fn yaml_normal_number(scan: &mut YamlScan<'_>, i: usize) -> usize {
    scan.flush("hl-plain");
    let end = yaml_number_end(scan.chars, i);
    let piece: String = scan.chars[i..end].iter().collect();
    scan.push_token("hl-num", &piece);
    *scan.line_start = false;
    end
}

fn yaml_normal_key(scan: &mut YamlScan<'_>, i: usize) -> usize {
    scan.flush("hl-plain");
    let end = yaml_key_end(scan.chars, i);
    let piece: String = scan.chars[i..end].iter().collect();
    let class = if KEYWORDS.contains(&piece.as_str()) {
        "hl-kw"
    } else if is_yaml_key_colon(scan.chars, end) {
        "hl-key"
    } else {
        "hl-plain"
    };
    scan.push_token(class, &piece);
    let mut next = end;
    if next < scan.chars.len() && scan.chars[next] == ':' {
        scan.push_token("hl-plain", ":");
        next += 1;
    }
    *scan.line_start = false;
    next
}

fn is_yaml_comment_hash(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j > 0 {
        j -= 1;
        match chars[j] {
            ' ' | '\t' => continue,
            ':' => return true,
            _ => return false,
        }
    }
    true
}

fn yaml_doc_marker_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len() && chars[i] != '\n' {
        i += 1;
    }
    i
}

fn yaml_anchor_end(chars: &[char], start: usize) -> usize {
    let mut i = start + 1;
    while i < chars.len()
        && (chars[i] == '_' || chars[i] == '-' || chars[i].is_ascii_alphanumeric())
    {
        i += 1;
    }
    i
}

fn yaml_tag_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ':' {
        i += 1;
    }
    i
}

fn is_yaml_key_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn yaml_key_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len()
        && (chars[i] == '_' || chars[i] == '-' || chars[i].is_ascii_alphanumeric())
    {
        i += 1;
    }
    i
}

fn is_yaml_key_colon(chars: &[char], i: usize) -> bool {
    let j = skip_ws(chars, i);
    j < chars.len() && chars[j] == ':'
}

fn skip_ws(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && matches!(chars[i], ' ' | '\t') {
        i += 1;
    }
    i
}

fn yaml_number_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    if i < chars.len() && chars[i] == '-' {
        i += 1;
    }
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
        i += 1;
    }
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_yaml_keys_and_comments() {
        let html = highlight_yaml_to_html("# c\nkey: value\n");
        assert!(html.contains("hl-com"));
        assert!(html.contains("hl-key"));
    }

    #[test]
    fn highlights_document_marker() {
        let html = highlight_yaml_to_html("---\n");
        assert!(html.contains("hl-attr"));
    }
}
