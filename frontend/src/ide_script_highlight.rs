//! 轻量 JS/TS/Go 语法高亮（C 族注释与字符串扫描）。

use crate::ide_highlight_common::push_span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Normal,
    LineComment,
    BlockComment,
    DoubleQuote,
    SingleQuote,
    Backtick,
}

struct Scan<'a> {
    chars: &'a [char],
    out: &'a mut String,
    buf: &'a mut String,
    keywords: &'static [&'static str],
    builtins: &'static [&'static str],
}

impl<'a> Scan<'a> {
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

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

fn classify_ident(scan: &mut Scan<'_>, word: &str) {
    if scan.keywords.contains(&word) {
        scan.push_token("hl-kw", word);
    } else if scan.builtins.contains(&word)
        || word.chars().next().is_some_and(|c| c.is_ascii_uppercase())
    {
        scan.push_token("hl-type", word);
    } else {
        scan.push_token("hl-plain", word);
    }
}

fn step_line_comment(scan: &mut Scan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\n' {
        scan.flush("hl-com");
        *state = State::Normal;
    }
    i + 1
}

fn step_block_comment(scan: &mut Scan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '*' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '/' {
        scan.buf.push(scan.chars[i + 1]);
        scan.flush("hl-com");
        *state = State::Normal;
        return i + 2;
    }
    i + 1
}

fn step_double_quote(scan: &mut Scan<'_>, i: usize, state: &mut State) -> usize {
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

fn step_single_quote(scan: &mut Scan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\\' && i + 1 < scan.chars.len() {
        scan.buf.push(scan.chars[i + 1]);
        return i + 2;
    }
    if c == '\'' {
        scan.flush("hl-str");
        *state = State::Normal;
    }
    i + 1
}

fn step_backtick(scan: &mut Scan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\\' && i + 1 < scan.chars.len() {
        scan.buf.push(scan.chars[i + 1]);
        return i + 2;
    }
    if c == '`' {
        scan.flush("hl-str");
        *state = State::Normal;
    }
    i + 1
}

fn step_normal(scan: &mut Scan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    if c == '/' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '/' {
        scan.buf.push(c);
        scan.buf.push(scan.chars[i + 1]);
        *state = State::LineComment;
        return i + 2;
    }
    if c == '/' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '*' {
        scan.buf.push(c);
        scan.buf.push(scan.chars[i + 1]);
        *state = State::BlockComment;
        return i + 2;
    }
    if c == '"' {
        scan.buf.push(c);
        *state = State::DoubleQuote;
        return i + 1;
    }
    if c == '\'' {
        scan.buf.push(c);
        *state = State::SingleQuote;
        return i + 1;
    }
    if c == '`' {
        scan.buf.push(c);
        *state = State::Backtick;
        return i + 1;
    }
    if c.is_ascii_digit()
        || (c == '.' && i + 1 < scan.chars.len() && scan.chars[i + 1].is_ascii_digit())
    {
        let start = i;
        let mut j = i;
        while j < scan.chars.len()
            && (scan.chars[j].is_ascii_digit()
                || scan.chars[j] == '.'
                || scan.chars[j] == 'x'
                || scan.chars[j] == 'X'
                || scan.chars[j] == 'e'
                || scan.chars[j] == 'E')
        {
            j += 1;
        }
        let num: String = scan.chars[start..j].iter().collect();
        scan.push_token("hl-num", &num);
        return j;
    }
    if is_ident_start(c) {
        let start = i;
        let mut j = i + 1;
        while j < scan.chars.len() && is_ident_part(scan.chars[j]) {
            j += 1;
        }
        let word: String = scan.chars[start..j].iter().collect();
        classify_ident(scan, &word);
        return j;
    }
    scan.buf.push(c);
    scan.flush("hl-plain");
    i + 1
}

fn highlight_script(
    source: &str,
    keywords: &'static [&'static str],
    builtins: &'static [&'static str],
) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let chars: Vec<char> = source.chars().collect();
    let mut buf = String::new();
    let mut state = State::Normal;
    let mut i = 0usize;
    while i < chars.len() {
        let mut scan = Scan {
            chars: &chars,
            out: &mut out,
            buf: &mut buf,
            keywords,
            builtins,
        };
        i = match state {
            State::LineComment => step_line_comment(&mut scan, i, &mut state),
            State::BlockComment => step_block_comment(&mut scan, i, &mut state),
            State::DoubleQuote => step_double_quote(&mut scan, i, &mut state),
            State::SingleQuote => step_single_quote(&mut scan, i, &mut state),
            State::Backtick => step_backtick(&mut scan, i, &mut state),
            State::Normal => step_normal(&mut scan, i, &mut state),
        };
    }
    let mut scan = Scan {
        chars: &chars,
        out: &mut out,
        buf: &mut buf,
        keywords,
        builtins,
    };
    match state {
        State::LineComment | State::BlockComment => scan.flush("hl-com"),
        State::DoubleQuote | State::SingleQuote | State::Backtick => scan.flush("hl-str"),
        State::Normal => scan.flush("hl-plain"),
    }
    out
}

const JS_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "await",
    "async",
    "of",
    "static",
];

const TS_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "await",
    "async",
    "of",
    "static",
    "interface",
    "type",
    "enum",
    "implements",
    "declare",
    "namespace",
    "module",
    "readonly",
    "private",
    "protected",
    "public",
    "abstract",
    "as",
    "any",
    "never",
    "unknown",
    "keyof",
    "infer",
    "satisfies",
];

const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

const GO_BUILTINS: &[&str] = &[
    "bool",
    "byte",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "true",
    "false",
    "iota",
    "nil",
    "len",
    "cap",
    "append",
    "make",
    "new",
    "panic",
    "recover",
    "print",
    "println",
];

#[must_use]
pub fn highlight_js_to_html(source: &str) -> String {
    highlight_script(source, JS_KEYWORDS, &[])
}

#[must_use]
pub fn highlight_ts_to_html(source: &str) -> String {
    highlight_script(source, TS_KEYWORDS, &[])
}

#[must_use]
pub fn highlight_go_to_html(source: &str) -> String {
    highlight_script(source, GO_KEYWORDS, GO_BUILTINS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_smoke() {
        assert!(highlight_js_to_html("const x = 1;").contains("hl-kw"));
    }

    #[test]
    fn go_smoke() {
        assert!(highlight_go_to_html("func main() {}").contains("hl-kw"));
    }
}
