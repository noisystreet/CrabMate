//! 轻量 Python 语法高亮（HTML span + CSS），供 IDE 编辑器镜像层使用。
//!
//! 覆盖：`#` 注释、单/双引号字符串（含前缀 `r`/`b`/`f`/`u` / `rb` / `br` / `fr` / `rf` 等，
//! 大小写不敏感）、三引号字符串、数字（含 `0x` / `0o` / `0b` / 浮点 / 复数 `j` 后缀 / 下划线分隔）、
//! 装饰器（`@name`）、关键字与内置类型/常量。

use crate::ide_highlight_common::push_span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Normal,
    LineComment,
    DoubleQuote,
    SingleQuote,
    TripleDouble,
    TripleSingle,
    /// f-string（普通双引号版本，内部不再细分格式段）。
    FString,
    /// f-string（三引号版本）。
    FStringTriple,
    /// 装饰器 `@name`：吸收后续标识符/点号。
    Decorator,
}

const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield", "match", "case", "type",
];

const BUILTIN_TYPES: &[&str] = &[
    "int",
    "float",
    "complex",
    "str",
    "bytes",
    "bytearray",
    "memoryview",
    "bool",
    "list",
    "tuple",
    "range",
    "set",
    "frozenset",
    "dict",
    "object",
    "type",
    "self",
    "cls",
    "Ellipsis",
    "NotImplemented",
];

const BUILTINS: &[&str] = &[
    "print",
    "len",
    "abs",
    "all",
    "any",
    "ascii",
    "bin",
    "callable",
    "chr",
    "compile",
    "delattr",
    "dir",
    "divmod",
    "eval",
    "exec",
    "filter",
    "format",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "isinstance",
    "issubclass",
    "iter",
    "locals",
    "map",
    "max",
    "min",
    "next",
    "oct",
    "open",
    "ord",
    "pow",
    "repr",
    "reversed",
    "round",
    "setattr",
    "slice",
    "sorted",
    "sum",
    "super",
    "vars",
    "zip",
];

#[must_use]
pub fn highlight_python_to_html(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let chars: Vec<char> = source.chars().collect();
    let mut buf = String::new();
    let mut state = State::Normal;
    let mut i = 0usize;
    while i < chars.len() {
        let mut scan = PyScan {
            chars: &chars,
            out: &mut out,
            buf: &mut buf,
        };
        i = match state {
            State::LineComment => py_step_line_comment(&mut scan, i, &mut state),
            State::DoubleQuote | State::FString => py_step_double_quote(&mut scan, i, &mut state),
            State::SingleQuote => py_step_single_quote(&mut scan, i, &mut state),
            State::TripleDouble | State::FStringTriple => {
                py_step_triple_double(&mut scan, i, &mut state)
            }
            State::TripleSingle => py_step_triple_single(&mut scan, i, &mut state),
            State::Decorator => py_step_decorator(&mut scan, i, &mut state),
            State::Normal => py_step_normal(&mut scan, i, &mut state),
        };
    }
    let mut scan = PyScan {
        chars: &chars,
        out: &mut out,
        buf: &mut buf,
    };
    match state {
        State::LineComment => scan.flush("hl-com"),
        State::DoubleQuote
        | State::SingleQuote
        | State::TripleDouble
        | State::TripleSingle
        | State::FString
        | State::FStringTriple => scan.flush("hl-str"),
        State::Decorator => scan.flush("hl-attr"),
        State::Normal => scan.flush("hl-plain"),
    }
    out
}

struct PyScan<'a> {
    chars: &'a [char],
    out: &'a mut String,
    buf: &'a mut String,
}

impl<'a> PyScan<'a> {
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

fn py_step_line_comment(scan: &mut PyScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\n' {
        scan.flush("hl-com");
        *state = State::Normal;
    }
    i + 1
}

fn py_step_double_quote(scan: &mut PyScan<'_>, i: usize, state: &mut State) -> usize {
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

fn py_step_single_quote(scan: &mut PyScan<'_>, i: usize, state: &mut State) -> usize {
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

fn py_step_triple_double(scan: &mut PyScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '"' && triple_double_at(scan.chars, i) {
        scan.buf.push(scan.chars[i + 1]);
        scan.buf.push(scan.chars[i + 2]);
        scan.flush("hl-str");
        *state = State::Normal;
        return i + 3;
    }
    i + 1
}

fn py_step_triple_single(scan: &mut PyScan<'_>, i: usize, state: &mut State) -> usize {
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

fn py_step_decorator(scan: &mut PyScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    // 装饰器行：吸收 `@name(.name)*`，直到行尾或空白。
    if c == '\n' {
        scan.flush("hl-attr");
        scan.out.push('\n');
        *state = State::Normal;
        return i + 1;
    }
    if c == '\\' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '\n' {
        scan.buf.push(c);
        scan.buf.push(scan.chars[i + 1]);
        return i + 2;
    }
    scan.buf.push(c);
    i + 1
}

fn py_step_normal(scan: &mut PyScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    // 注释
    if c == '#' {
        scan.flush("hl-plain");
        scan.buf.push('#');
        *state = State::LineComment;
        return i + 1;
    }
    // 装饰器：行首（允许前置空白）的 `@`
    if c == '@' && at_decorator_pos(scan.chars, i) {
        scan.flush("hl-plain");
        scan.buf.push('@');
        *state = State::Decorator;
        return i + 1;
    }
    // 标识符 / 字符串前缀
    if c.is_ascii_alphabetic() || c == '_' {
        return py_step_normal_ident(scan, i, state);
    }
    // 三引号字符串（无前缀）
    if triple_double_at(scan.chars, i) {
        scan.flush("hl-plain");
        scan.buf.push_str("\"\"\"");
        *state = State::TripleDouble;
        return i + 3;
    }
    if triple_single_at(scan.chars, i) {
        scan.flush("hl-plain");
        scan.buf.push_str("'''");
        *state = State::TripleSingle;
        return i + 3;
    }
    // 数字
    if c.is_ascii_digit()
        || (c == '.' && i + 1 < scan.chars.len() && scan.chars[i + 1].is_ascii_digit())
    {
        scan.flush("hl-plain");
        let end = number_end(scan.chars, i);
        let piece: String = scan.chars[i..end].iter().collect();
        scan.push_token("hl-num", &piece);
        return end;
    }
    // 单/双引号
    if c == '"' {
        scan.flush("hl-plain");
        scan.buf.push('"');
        *state = State::DoubleQuote;
        return i + 1;
    }
    if c == '\'' {
        scan.flush("hl-plain");
        scan.buf.push('\'');
        *state = State::SingleQuote;
        return i + 1;
    }
    scan.buf.push(c);
    i + 1
}

fn py_step_normal_ident(scan: &mut PyScan<'_>, i: usize, state: &mut State) -> usize {
    let end = ident_end(scan.chars, i);
    let word: String = scan.chars[i..end].iter().collect();
    let lower = word.to_ascii_lowercase();
    // 三引号 + 前缀
    if end + 2 < scan.chars.len()
        && scan.chars[end] == '"'
        && scan.chars[end + 1] == '"'
        && scan.chars[end + 2] == '"'
        && is_string_prefix(&lower)
    {
        scan.flush("hl-plain");
        scan.buf.push_str(&word);
        scan.buf.push_str("\"\"\"");
        *state = if lower.contains('f') {
            State::FStringTriple
        } else {
            State::TripleDouble
        };
        return end + 3;
    }
    if end + 2 < scan.chars.len()
        && scan.chars[end] == '\''
        && scan.chars[end + 1] == '\''
        && scan.chars[end + 2] == '\''
        && is_string_prefix(&lower)
    {
        scan.flush("hl-plain");
        scan.buf.push_str(&word);
        scan.buf.push_str("'''");
        *state = State::TripleSingle;
        return end + 3;
    }
    // 普通字符串 + 前缀
    if end < scan.chars.len() && scan.chars[end] == '"' && is_string_prefix(&lower) {
        scan.flush("hl-plain");
        scan.buf.push_str(&word);
        scan.buf.push('"');
        *state = if lower.contains('f') {
            State::FString
        } else {
            State::DoubleQuote
        };
        return end + 1;
    }
    if end < scan.chars.len() && scan.chars[end] == '\'' && is_string_prefix(&lower) {
        scan.flush("hl-plain");
        scan.buf.push_str(&word);
        scan.buf.push('\'');
        *state = State::SingleQuote;
        return end + 1;
    }
    // 普通标识符/关键字
    scan.flush("hl-plain");
    let class = if KEYWORDS.contains(&word.as_str()) {
        "hl-kw"
    } else if BUILTIN_TYPES.contains(&word.as_str()) {
        "hl-type"
    } else if BUILTINS.contains(&word.as_str()) {
        "hl-attr"
    } else {
        "hl-plain"
    };
    scan.push_token(class, &word);
    end
}

fn at_decorator_pos(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j > 0 {
        j -= 1;
        match chars[j] {
            ' ' | '\t' => continue,
            '\n' => return true,
            // 装饰器后接标识符字符：保留原状态（应已被 Decorator 态吸收）。
            _ => return false,
        }
    }
    true
}

fn is_string_prefix(lower_word: &str) -> bool {
    // 允许空字符串（无前缀）也返回 true，但调用方一般不会传空。
    if lower_word.is_empty() {
        return true;
    }
    matches!(
        lower_word,
        "r" | "b" | "f" | "u" | "rb" | "br" | "rf" | "fr" | "ur" | "ru"
    )
}

fn triple_double_at(chars: &[char], i: usize) -> bool {
    i + 2 < chars.len() && chars[i] == '"' && chars[i + 1] == '"' && chars[i + 2] == '"'
}

fn triple_single_at(chars: &[char], i: usize) -> bool {
    i + 2 < chars.len() && chars[i] == '\'' && chars[i + 1] == '\'' && chars[i + 2] == '\''
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
    // 十六进制 / 八进制 / 二进制
    if i + 1 < chars.len() && chars[i] == '0' {
        let next = chars[i + 1];
        if next == 'x' || next == 'X' {
            i += 2;
            while i < chars.len() && (chars[i].is_ascii_hexdigit() || chars[i] == '_') {
                i += 1;
            }
            return consume_complex_suffix(chars, i);
        }
        if next == 'o' || next == 'O' {
            i += 2;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                i += 1;
            }
            return consume_complex_suffix(chars, i);
        }
        if next == 'b' || next == 'B' {
            i += 2;
            while i < chars.len() && (chars[i] == '0' || chars[i] == '1' || chars[i] == '_') {
                i += 1;
            }
            return consume_complex_suffix(chars, i);
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
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
            any = true;
            i += 1;
        }
        if !any {
            i = t;
        }
    }
    consume_complex_suffix(chars, i)
}

fn consume_complex_suffix(chars: &[char], start: usize) -> usize {
    let mut i = start;
    // Python 数字字面量末尾允许 `j` / `J`（复数）。
    if i < chars.len() && (chars[i] == 'j' || chars[i] == 'J') {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_keywords_and_comments() {
        let html = highlight_python_to_html("def f():\n    # hi\n    return 1");
        assert!(html.contains("hl-kw"));
        assert!(html.contains("def"));
        assert!(html.contains("return"));
        assert!(html.contains("hl-com"));
        assert!(html.contains("# hi"));
    }

    #[test]
    fn highlights_strings_and_fstring() {
        let html = highlight_python_to_html(r#"s = "a\"b" + f"x={y}""#);
        assert!(html.contains("hl-str"));
    }

    #[test]
    fn highlights_triple_quoted() {
        let html = highlight_python_to_html("\"\"\"\nmulti\nline\n\"\"\"\n");
        assert!(html.contains("hl-str"));
        assert!(html.contains("multi"));
    }

    #[test]
    fn highlights_decorator() {
        let html = highlight_python_to_html("@property\ndef x(self):\n    pass\n");
        assert!(html.contains("hl-attr"));
        assert!(html.contains("@property"));
    }

    #[test]
    fn highlights_numbers() {
        let html = highlight_python_to_html("x = 0xFF + 0b101 + 1_000 + 3.14j");
        assert!(html.contains("hl-num"));
        assert!(html.contains("0xFF"));
        assert!(html.contains("3.14j"));
    }

    #[test]
    fn escapes_html_in_source() {
        let html = highlight_python_to_html("a < b");
        assert!(html.contains("&lt;"));
        assert!(!html.contains("< b"));
    }
}
