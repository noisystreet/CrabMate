//! 轻量 C/C++ 语法高亮（HTML span + CSS），供 IDE 编辑器镜像层使用。
//!
//! 共享扫描器：C++ 是 C 的超集，按 `Dialect` 选择关键字集合。
//! 覆盖：行/块注释、单/双引号字符串、宽/UTF 串前缀（`L"..."` / `u8"..."` / `U"..."` / `u"..."`）、
//! 原始字符串（`R"(...)"` / `LR"(...)"` 等）、`#` 预处理指令、`#include <header>`、
//! 数字（含十六进制/八进制/二进制与 `u/U/l/L/ul/UL/f/F` 后缀）。

use crate::ide_highlight_common::push_span;

/// C/C++ 方言：用于选择关键字集合与少量差异。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CppDialect {
    C,
    Cpp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Normal,
    LineComment,
    BlockComment,
    DoubleQuote,
    SingleQuote,
    /// 已进入 `#` 预处理行（到行尾或 `\`-续行前）。
    Preproc,
    /// `#include` 之后等待 `<...>` 头文件名。
    PreprocInclude,
    /// 头文件名 `<...>` 内部。
    HeaderName,
    /// 原始字符串 `R"delim(...)delim"`：记录 `(` 后等待的 delimiter。
    RawString {
        delim: String,
    },
}

const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "_Bool",
    "_Complex",
    "_Imaginary",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Generic",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
];

const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "and_eq",
    "asm",
    "auto",
    "bitand",
    "bitor",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "char8_t",
    "char16_t",
    "char32_t",
    "class",
    "compl",
    "concept",
    "const",
    "consteval",
    "constexpr",
    "constinit",
    "const_cast",
    "continue",
    "co_await",
    "co_return",
    "co_yield",
    "decltype",
    "default",
    "delete",
    "do",
    "double",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "final",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "nullptr",
    "operator",
    "or",
    "or_eq",
    "override",
    "private",
    "protected",
    "public",
    "register",
    "reinterpret_cast",
    "requires",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "wchar_t",
    "while",
    "xor",
    "xor_eq",
];

const C_BUILTIN_TYPES: &[&str] = &[
    "size_t",
    "ssize_t",
    "ptrdiff_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "FILE",
    "fpos_t",
    "clock_t",
    "time_t",
    "wchar_t",
    "bool",
];

const CPP_BUILTIN_TYPES: &[&str] = &[
    "std",
    "string",
    "wstring",
    "u16string",
    "u32string",
    "string_view",
    "vector",
    "map",
    "unordered_map",
    "multimap",
    "unordered_multimap",
    "set",
    "unordered_set",
    "multiset",
    "unordered_multiset",
    "deque",
    "list",
    "forward_list",
    "array",
    "tuple",
    "pair",
    "optional",
    "variant",
    "any",
    "shared_ptr",
    "unique_ptr",
    "weak_ptr",
    "function",
    "bitset",
    "queue",
    "priority_queue",
    "stack",
    "span",
    "size_t",
    "ptrdiff_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "nullptr_t",
    "byte",
];

/// 字符串/原始串前缀（出现在引号前；区分大小写）。
const STRING_PREFIXES: &[&str] = &["u8", "u", "U", "L"];
const RAW_PREFIXES: &[&str] = &["R", "LR", "u8R", "uR", "UR"];

#[must_use]
pub fn highlight_c_to_html(source: &str) -> String {
    highlight_c_cpp_to_html(source, CppDialect::C)
}

#[must_use]
pub fn highlight_cpp_to_html(source: &str) -> String {
    highlight_c_cpp_to_html(source, CppDialect::Cpp)
}

struct CScan<'a> {
    chars: &'a [char],
    out: &'a mut String,
    buf: &'a mut String,
    keywords: &'static [&'static str],
    builtins: &'static [&'static str],
}

impl<'a> CScan<'a> {
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
pub fn highlight_c_cpp_to_html(source: &str, dialect: CppDialect) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let chars: Vec<char> = source.chars().collect();
    let (keywords, builtins) = match dialect {
        CppDialect::C => (C_KEYWORDS, C_BUILTIN_TYPES),
        CppDialect::Cpp => (CPP_KEYWORDS, CPP_BUILTIN_TYPES),
    };
    let mut buf = String::new();
    let mut state = State::Normal;
    let mut i = 0usize;
    while i < chars.len() {
        let mut scan = CScan {
            chars: &chars,
            out: &mut out,
            buf: &mut buf,
            keywords,
            builtins,
        };
        i = if let State::RawString { delim } = &state {
            let delim = delim.clone();
            c_step_raw_string(&mut scan, i, &mut state, &delim)
        } else {
            match &state {
                State::LineComment => c_step_line_comment(&mut scan, i, &mut state),
                State::BlockComment => c_step_block_comment(&mut scan, i, &mut state),
                State::DoubleQuote => c_step_double_quote(&mut scan, i, &mut state),
                State::SingleQuote => c_step_single_quote(&mut scan, i, &mut state),
                State::Preproc => c_step_preproc(&mut scan, i, &mut state),
                State::PreprocInclude => c_step_preproc_include(&mut scan, i, &mut state),
                State::HeaderName => c_step_header_name(&mut scan, i, &mut state),
                State::RawString { .. } => unreachable!(),
                State::Normal => c_step_normal(&mut scan, i, &mut state),
            }
        };
    }
    let mut scan = CScan {
        chars: &chars,
        out: &mut out,
        buf: &mut buf,
        keywords,
        builtins,
    };
    match state {
        State::LineComment | State::BlockComment => scan.flush("hl-com"),
        State::DoubleQuote | State::SingleQuote | State::RawString { .. } | State::HeaderName => {
            scan.flush("hl-str")
        }
        State::Preproc | State::PreprocInclude => scan.flush("hl-mac"),
        State::Normal => scan.flush("hl-plain"),
    }
    out
}

fn c_step_line_comment(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '\n' {
        scan.flush("hl-com");
        *state = State::Normal;
    }
    i + 1
}

fn c_step_block_comment(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
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

fn c_step_double_quote(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
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

fn c_step_single_quote(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
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

fn c_step_preproc(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    // `\`-续行
    if c == '\\' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '\n' {
        scan.buf.push(c);
        scan.buf.push(scan.chars[i + 1]);
        return i + 2;
    }
    if c == '\n' {
        scan.flush("hl-mac");
        scan.out.push('\n');
        *state = State::Normal;
        return i + 1;
    }
    // `#include` 之后遇到 `<`：把 `<...>` 当头文件名高亮。
    if c == '<' && is_include_directive(scan.buf) {
        scan.flush("hl-mac");
        scan.buf.push('<');
        *state = State::HeaderName;
        return i + 1;
    }
    // 预处理行内的字符串/注释：与正文一致，但跨状态机较繁琐，
    // 这里保持简单——仅在预处理状态中按字面字符累积（含 `"` 时切到字符串态）。
    if c == '"' {
        scan.flush("hl-mac");
        scan.buf.push('"');
        *state = State::DoubleQuote;
        return i + 1;
    }
    if c == '/' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '/' {
        scan.flush("hl-mac");
        scan.buf.push('/');
        scan.buf.push('/');
        *state = State::LineComment;
        return i + 2;
    }
    scan.buf.push(c);
    i + 1
}

fn c_step_preproc_include(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    if c.is_whitespace() {
        scan.out.push(c);
        return i + 1;
    }
    if c == '<' {
        scan.buf.push('<');
        *state = State::HeaderName;
        return i + 1;
    }
    if c == '"' {
        scan.flush("hl-mac");
        scan.buf.push('"');
        *state = State::DoubleQuote;
        return i + 1;
    }
    // 其它：退回预处理状态。
    *state = State::Preproc;
    i
}

fn c_step_header_name(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == '>' || c == '\n' {
        scan.flush("hl-str");
        *state = State::Normal;
    }
    i + 1
}

fn c_step_raw_string(scan: &mut CScan<'_>, i: usize, state: &mut State, delim: &str) -> usize {
    let c = scan.chars[i];
    scan.buf.push(c);
    if c == ')' {
        let delim_chars: Vec<char> = delim.chars().collect();
        let mut j = i + 1;
        let mut matched = 0usize;
        while j < scan.chars.len()
            && matched < delim_chars.len()
            && scan.chars[j] == delim_chars[matched]
        {
            j += 1;
            matched += 1;
        }
        if matched == delim_chars.len() && j < scan.chars.len() && scan.chars[j] == '"' {
            scan.buf.push_str(delim);
            scan.buf.push('"');
            scan.flush("hl-str");
            *state = State::Normal;
            return j + 1;
        }
    }
    i + 1
}

fn c_step_normal(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
    let c = scan.chars[i];
    // 行注释
    if c == '/' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '/' {
        scan.flush("hl-plain");
        scan.buf.push('/');
        scan.buf.push('/');
        *state = State::LineComment;
        return i + 2;
    }
    // 块注释
    if c == '/' && i + 1 < scan.chars.len() && scan.chars[i + 1] == '*' {
        scan.flush("hl-plain");
        scan.buf.push('/');
        scan.buf.push('*');
        *state = State::BlockComment;
        return i + 2;
    }
    // 预处理指令：行首（跳过空白）的 `#`
    if c == '#' && at_line_start(scan.chars, i) {
        return c_step_normal_hash(scan, i, state);
    }
    // 标识符 / 原始串前缀 / 字符串前缀
    if c.is_ascii_alphabetic() || c == '_' {
        return c_step_normal_ident(scan, i, state);
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
    // 字符串
    if c == '"' {
        scan.flush("hl-plain");
        scan.buf.push('"');
        *state = State::DoubleQuote;
        return i + 1;
    }
    // 字符
    if c == '\'' {
        scan.flush("hl-plain");
        scan.buf.push('\'');
        *state = State::SingleQuote;
        return i + 1;
    }
    scan.buf.push(c);
    i + 1
}

fn c_step_normal_hash(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
    scan.flush("hl-plain");
    scan.buf.push('#');
    // 推进到 include 关键字或结束
    let mut j = i + 1;
    while j < scan.chars.len() && scan.chars[j].is_whitespace() {
        scan.buf.push(scan.chars[j]);
        j += 1;
    }
    let ident_end_pos = ident_end(scan.chars, j);
    if ident_end_pos > j {
        let word: String = scan.chars[j..ident_end_pos].iter().collect();
        scan.buf.push_str(&word);
        if word == "include" {
            scan.flush("hl-mac");
            *state = State::PreprocInclude;
            return ident_end_pos;
        }
        *state = State::Preproc;
        return ident_end_pos;
    }
    *state = State::Preproc;
    j
}

fn c_step_normal_ident(scan: &mut CScan<'_>, i: usize, state: &mut State) -> usize {
    let end = ident_end(scan.chars, i);
    let word: String = scan.chars[i..end].iter().collect();
    // 原始字符串前缀（R / LR / u8R / uR / UR）后跟 `"`
    if end < scan.chars.len() && scan.chars[end] == '"' && RAW_PREFIXES.contains(&word.as_str()) {
        return c_step_normal_raw_string(scan, end, state, &word);
    }
    // 字符串前缀（u8/u/U/L）后跟 `"`
    if end < scan.chars.len() && scan.chars[end] == '"' && STRING_PREFIXES.contains(&word.as_str())
    {
        scan.flush("hl-plain");
        scan.buf.push_str(&word);
        scan.buf.push('"');
        *state = State::DoubleQuote;
        return end + 1;
    }
    // 普通标识符/关键字
    scan.flush("hl-plain");
    let class = if scan.keywords.contains(&word.as_str()) {
        "hl-kw"
    } else if scan.builtins.contains(&word.as_str()) {
        "hl-type"
    } else {
        "hl-plain"
    };
    scan.push_token(class, &word);
    end
}

fn c_step_normal_raw_string(
    scan: &mut CScan<'_>,
    quote_i: usize,
    state: &mut State,
    prefix: &str,
) -> usize {
    scan.flush("hl-plain");
    scan.buf.push_str(prefix);
    scan.buf.push('"');
    // 收集 delimiter：`"delim(... )delim"`
    let mut j = quote_i + 1;
    let mut delim = String::new();
    while j < scan.chars.len() && scan.chars[j] != '(' {
        if scan.chars[j] == '\n' {
            break;
        }
        delim.push(scan.chars[j]);
        j += 1;
    }
    if j < scan.chars.len() && scan.chars[j] == '(' {
        scan.buf.push_str(&delim);
        scan.buf.push('(');
        j += 1;
    } else {
        // 不正常的原始串，按普通字符串处理
        scan.flush("hl-str");
        *state = State::Normal;
        return j;
    }
    *state = State::RawString {
        delim: delim.clone(),
    };
    j
}

fn at_line_start(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j > 0 {
        j -= 1;
        match chars[j] {
            ' ' | '\t' => continue,
            '\n' => return true,
            _ => return false,
        }
    }
    true
}

fn is_include_directive(buf: &str) -> bool {
    let trimmed = buf.trim_start_matches(|c: char| c.is_whitespace() || c == '#');
    let head = trimmed.split_whitespace().next().unwrap_or("");
    head == "include"
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
    if i + 1 < chars.len() && chars[i] == '0' {
        let next = chars[i + 1];
        if next == 'x' || next == 'X' {
            i += 2;
            while i < chars.len() && (chars[i].is_ascii_hexdigit() || chars[i] == '\'') {
                i += 1;
            }
            return consume_number_suffix(chars, i);
        }
        if next == 'b' || next == 'B' {
            i += 2;
            while i < chars.len() && (chars[i] == '0' || chars[i] == '1' || chars[i] == '\'') {
                i += 1;
            }
            return consume_number_suffix(chars, i);
        }
    }
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '\'') {
        i += 1;
    }
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '\'') {
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
        if !any {
            i = t;
        }
    }
    consume_number_suffix(chars, i)
}

fn consume_number_suffix(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len() && matches!(chars[i], 'u' | 'U' | 'l' | 'L' | 'f' | 'F' | 'z' | 'Z') {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_c_keywords_and_comments() {
        let html = highlight_c_to_html("int main() { // hi\nreturn 0; }");
        assert!(html.contains("hl-kw"));
        assert!(html.contains("int"));
        assert!(html.contains("hl-com"));
        assert!(html.contains("// hi"));
    }

    #[test]
    fn highlights_cpp_class_template() {
        let html = highlight_cpp_to_html("template <class T> struct Foo {};");
        assert!(html.contains("hl-kw"));
        assert!(html.contains("template"));
        assert!(html.contains("struct"));
    }

    #[test]
    fn highlights_preprocessor_include() {
        let html = highlight_c_to_html("#include <stdio.h>\n");
        assert!(html.contains("hl-mac"));
        assert!(html.contains("hl-str"));
        // 头文件名 `<stdio.h>` 经 HTML 转义后为 `&lt;stdio.h&gt;`。
        assert!(html.contains("&lt;stdio.h&gt;"));
    }

    #[test]
    fn highlights_raw_string_cpp() {
        let html = highlight_cpp_to_html(r#"R"(raw "text")"#);
        assert!(html.contains("hl-str"));
    }

    #[test]
    fn highlights_numbers_with_suffix() {
        let html = highlight_cpp_to_html("auto x = 42UL + 3.14f;");
        assert!(html.contains("hl-num"));
        assert!(html.contains("42UL"));
        assert!(html.contains("3.14f"));
    }

    #[test]
    fn escapes_html_in_source() {
        let html = highlight_c_to_html("int a < b;");
        assert!(html.contains("&lt;"));
        assert!(!html.contains("< b"));
    }

    #[test]
    fn highlights_block_comment() {
        let html = highlight_c_to_html("/* multi\nline */\nint x;");
        assert!(html.contains("hl-com"));
        assert!(html.contains("multi"));
    }
}
