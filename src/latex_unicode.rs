//! 将常见 LaTeX 数学定界符内的内容转为 Unicode，供终端 Markdown 渲染（`unicodeit`）。
//!
//! 与 Web 端数学公式展示无关；TUI / `api` 流式终端输出共用此逻辑。
//!
//! 在 `unicodeit` 之前做**小规模结构化预处理**：`\frac`、`\\sqrt`、`\text`/`\mathrm` 等拆壳、`\left`/`\right` 剥离、`\quad` 等空白命令。

use regex::Regex;
use unicodeit::replace as unicodeit_replace;

/// 文本/字体类命令：只剥一层 `{…}`，内层不再含下列前缀时才替换（从最内层往外剥）。
const TEXT_STYLE_CMDS: &[&str] = &[
    "\\operatorname",
    "\\mathrm",
    "\\mathbf",
    "\\mathit",
    "\\mathsf",
    "\\mathcal",
    "\\textrm",
    "\\mbox",
    "\\text",
];

const MAX_STRUCTURE_PASSES: usize = 4096;

/// `open_idx` 指向 `{`，返回内容与闭合 `}` 之后下标。
fn parse_balanced_brace(s: &str, open_idx: usize) -> Option<(&str, usize)> {
    let bytes = s.as_bytes();
    if open_idx >= bytes.len() || bytes[open_idx] != b'{' {
        return None;
    }
    let mut depth = 1usize;
    let start_inner = open_idx + 1;
    let mut i = start_inner;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((s.get(start_inner..i)?, i + 1));
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

fn inner_has_text_style_wrapper(inner: &str) -> bool {
    TEXT_STYLE_CMDS.iter().any(|c| inner.contains(*c))
}

/// `pos` 起为 `cmd`（如 `\mathrm`），可选 `\operatorname*` 的 `*`，再读一对 `{…}`。
fn try_parse_cmd_brace<'a>(s: &'a str, pos: usize, cmd: &str) -> Option<(&'a str, usize, usize)> {
    let rest = s.get(pos..)?;
    if !rest.starts_with(cmd) {
        return None;
    }
    let mut j = pos + cmd.len();
    let bytes = s.as_bytes();
    if cmd == "\\operatorname" && j < bytes.len() && bytes[j] == b'*' {
        j += 1;
    }
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    let (inner, end) = parse_balanced_brace(s, j)?;
    Some((inner, pos, end))
}

fn preprocess_text_wrappers(s: &str) -> String {
    let mut cur = s.to_string();
    for _ in 0..MAX_STRUCTURE_PASSES {
        let mut replaced = false;
        let mut search = 0usize;
        'scan: while search < cur.len() {
            let mut best: Option<(usize, &str)> = None;
            for cmd in TEXT_STYLE_CMDS {
                if let Some(rel) = cur[search..].find(*cmd) {
                    let pos = search + rel;
                    if best.is_none_or(|(p, _)| pos < p) {
                        best = Some((pos, *cmd));
                    }
                }
            }
            let Some((pos, cmd)) = best else {
                break;
            };
            if let Some((inner, start, end)) = try_parse_cmd_brace(&cur, pos, cmd)
                && !inner_has_text_style_wrapper(inner)
            {
                let replacement = inner.trim().to_string();
                cur.replace_range(start..end, &replacement);
                replaced = true;
                break 'scan;
            }
            search = pos + 1;
        }
        if !replaced {
            break;
        }
    }
    cur
}

/// 解析 `\sqrt[n]{body}`（`n` 段无 `{`）或 `\sqrt{body}`。`body` 内仍含 `\sqrt` 时不处理（留给更内层）。
fn try_parse_sqrt(s: &str, pos: usize) -> Option<(String, usize, usize)> {
    let rest = s.get(pos..)?;
    if !rest.starts_with("\\sqrt") {
        return None;
    }
    let after = pos + "\\sqrt".len();
    let bytes = s.as_bytes();
    if after < bytes.len() && bytes[after].is_ascii_alphabetic() {
        return None;
    }
    let mut j = after;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    if j >= bytes.len() {
        return None;
    }

    let (n_opt, k) = if bytes[j] == b'[' {
        let n_start = j + 1;
        let close_rel = s.get(n_start..)?.find(']')?;
        let n_end = n_start + close_rel;
        let n_raw = s.get(n_start..n_end)?;
        if n_raw.contains('{') || n_raw.contains('}') {
            return None;
        }
        let mut after_br = n_end + 1;
        while after_br < bytes.len() && bytes[after_br].is_ascii_whitespace() {
            after_br += 1;
        }
        (Some(n_raw.trim()), after_br)
    } else {
        (None, j)
    };

    let (body, end) = parse_balanced_brace(s, k)?;
    if body.contains("\\sqrt") {
        return None;
    }
    let inner = body.trim();
    let body_out = if inner.len() == 1 && inner.chars().next().is_some_and(|c| c.is_alphanumeric())
    {
        inner.to_string()
    } else {
        format!("({})", inner)
    };

    let rep = match n_opt {
        None | Some("2") | Some("") => format!("\u{221a}{}", body_out),
        Some("3") => format!("\u{221b}{}", body_out),
        Some("4") => format!("\u{221c}{}", body_out),
        Some(n) => format!("root({},{})", n, body_out),
    };
    Some((rep, pos, end))
}

fn preprocess_sqrt(s: &str) -> String {
    let mut cur = s.to_string();
    for _ in 0..MAX_STRUCTURE_PASSES {
        let mut replaced = false;
        let mut search = 0usize;
        while search < cur.len() {
            let Some(rel) = cur[search..].find("\\sqrt") else {
                break;
            };
            let pos = search + rel;
            if let Some((rep, start, end)) = try_parse_sqrt(&cur, pos) {
                cur.replace_range(start..end, &rep);
                replaced = true;
                break;
            }
            search = pos + 1;
        }
        if !replaced {
            break;
        }
    }
    cur
}

/// 自 `pos` 起解析 `\frac{…}{…}`（允许 `\frac` 与首括号间空白），失败则 `None`。
fn try_parse_frac(s: &str, pos: usize) -> Option<(&str, &str, usize, usize)> {
    let rest = s.get(pos..)?;
    if !rest.starts_with("\\frac") {
        return None;
    }
    let mut j = pos + "\\frac".len();
    let bytes = s.as_bytes();
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    let (num, after_num) = parse_balanced_brace(s, j)?;
    let mut k = after_num;
    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
        k += 1;
    }
    let (den, after_den) = parse_balanced_brace(s, k)?;
    Some((num, den, pos, after_den))
}

fn frac_to_linear(num: &str, den: &str) -> String {
    let num = num.trim();
    let den = den.trim();
    let num_out = if num.contains('/') {
        format!("({})", num)
    } else {
        num.to_string()
    };
    let den_out = if den.contains('/') {
        format!("({})", den)
    } else {
        den.to_string()
    };
    format!("{}/{}", num_out, den_out)
}

fn preprocess_frac(s: &str) -> String {
    let mut cur = s.to_string();
    for _ in 0..MAX_STRUCTURE_PASSES {
        let mut replaced = false;
        let mut search = 0usize;
        while search < cur.len() {
            let Some(rel) = cur[search..].find("\\frac") else {
                break;
            };
            let pos = search + rel;
            if let Some((num, den, start, end)) = try_parse_frac(&cur, pos)
                && !num.contains("\\frac")
                && !den.contains("\\frac")
            {
                let linear = frac_to_linear(num, den);
                cur.replace_range(start..end, &linear);
                replaced = true;
                break;
            }
            search = pos + 1;
        }
        if !replaced {
            break;
        }
    }
    cur
}

fn preprocess_left_right(s: &str) -> String {
    let mut x = s.to_string();
    for (from, to) in [
        ("\\left(", "("),
        ("\\right)", ")"),
        ("\\left[", "["),
        ("\\right]", "]"),
        ("\\left\\{", "{"),
        ("\\right\\}", "}"),
        ("\\left|", "|"),
        ("\\right|", "|"),
        ("\\left\\lVert", "‖"),
        ("\\right\\rVert", "‖"),
    ] {
        x = x.replace(from, to);
    }
    x
}

fn preprocess_spacing_cmds(s: &str) -> String {
    s.replace("\\qquad", "  ")
        .replace("\\quad", " ")
        .replace("\\;", " ")
        .replace("\\:", " ")
        .replace("\\,", " ")
}

/// 定界符内：文本类拆壳 → `\\sqrt` → `\\frac` → `\left`/`\right` → 空白命令，最后交给 `unicodeit`。
fn preprocess_latex_structure(s: &str) -> String {
    let x = preprocess_text_wrappers(s);
    let x = preprocess_sqrt(&x);
    let x = preprocess_frac(&x);
    let x = preprocess_left_right(&x);
    preprocess_spacing_cmds(&x)
}

/// 将文本中的 LaTeX 数学公式（`$...$`、`$$...$$`、`\(...\)`、`\[...\]`）转为 Unicode，便于终端显示。
pub fn latex_math_to_unicode(s: &str) -> String {
    // 顺序：先 display 块再行内，避免 `$` 与 `\(` 边界歧义
    let patterns = [
        r"\\\[([\s\S]*?)\\\]", // \[ ... \]
        r"\\\(([\s\S]*?)\\\)", // \( ... \)
        r"\$\$([\s\S]*?)\$\$", // $$ ... $$
        r"\$([^$\n]+)\$",      // $ ... $（单行）
    ];
    let mut out = s.to_string();
    for pat in patterns {
        if let Ok(re) = Regex::new(pat) {
            out = re
                .replace_all(&out, |caps: &regex::Captures<'_>| {
                    let inner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                    let pre = preprocess_latex_structure(inner.trim());
                    unicodeit_replace(&pre)
                })
                .into_owned();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latex_math_to_unicode_inline() {
        let out = latex_math_to_unicode("公式 $x^2$ 结束");
        assert!(!out.contains("$"));
        assert!(out.contains('x') || out.contains('²'));
    }

    #[test]
    fn test_latex_math_to_unicode_display() {
        let out = latex_math_to_unicode("$$1+1=2$$");
        assert!(!out.contains("$$"));
    }

    #[test]
    fn test_latex_math_to_unicode_plain_unchanged() {
        let s = "纯文本无公式";
        assert_eq!(latex_math_to_unicode(s), s);
    }

    #[test]
    fn test_frac_simple_to_slash() {
        let out = latex_math_to_unicode(r"$\frac{1}{2}$");
        assert!(!out.contains('$'), "out={out:?}");
        assert!(out.contains("1/2"), "expected 1/2 in {out:?}");
    }

    #[test]
    fn test_frac_nested_parentheses() {
        let out = latex_math_to_unicode(r"$\frac{\frac{1}{2}}{3}$");
        assert!(out.contains("(1/2)/3"), "expected (1/2)/3 in {out:?}");
    }

    #[test]
    fn test_preprocess_frac_leaves_sfrac_for_unicodeit() {
        let out = latex_math_to_unicode(r"$\sfrac{1}{2}$");
        assert!(!out.contains('$'));
        assert!(!out.contains('\\'));
        assert!(out.contains('½') || out.contains("1/2") || out.contains('2'));
    }

    #[test]
    fn test_sqrt_square() {
        let out = latex_math_to_unicode(r"$\sqrt{x+1}$");
        assert!(!out.contains('\\'));
        assert!(out.contains('\u{221a}'), "out={out:?}");
    }

    #[test]
    fn test_sqrt_cube() {
        let out = latex_math_to_unicode(r"$\sqrt[3]{8}$");
        assert!(!out.contains('\\'));
        assert!(out.contains('\u{221b}'), "out={out:?}");
    }

    #[test]
    fn test_text_unwrap() {
        let out = latex_math_to_unicode(r"$\text{if } x>0$");
        assert!(!out.contains("$"));
        assert!(!out.contains("\\text"));
    }

    #[test]
    fn test_left_right_strip() {
        let out = latex_math_to_unicode(r"$\left(\frac{1}{2}\right)$");
        assert!(!out.contains("\\left"));
        assert!(!out.contains("\\right"));
    }

    #[test]
    fn test_quad_collapses() {
        let out = latex_math_to_unicode("$a\\quad b$");
        assert!(!out.contains("\\quad"));
    }
}
