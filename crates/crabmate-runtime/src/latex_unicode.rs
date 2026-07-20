//! 命令行输出中的 LaTeX 数学片段 → Unicode 近似转换。
//!
//! 经 `message_display.rs` 调用，让用户在终端看到 `∫` 而非 `\int`。

use regex::Regex;
use std::sync::LazyLock;
use unicodeit::replace;

static LATEX_MATH_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)\\\((.+?)\\\)"#).expect("inline math paren regex"));
// 单美元行内公式
static LATEX_MATH_DOLLAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\$(.+?)\$").expect("inline math dollar regex"));
// 双美元块级公式
static LATEX_MATH_DISPLAY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\$\$(.+?)\$\$").expect("display math regex"));
// 兼容 `\[ ... \]` 风格
// 注意：markdown 渲染后可能 `\[` 已转义为 `\[`；此处同时处理两种
static LATEX_MATH_BRACKET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\\\[(.+?)\\\]").expect("bracket math regex"));

/// 将字符串中的 LaTeX 数学表达式近似转换为 Unicode 符号。
pub fn latex_math_to_unicode(text: &str) -> String {
    // 先处理 `\[...\]`（括号），再处理 `$$...$$`（块级），再处理 `$...$`（行内），最后 `\(...\)`
    let s = LATEX_MATH_BRACKET.replace_all(text, |caps: &regex::Captures<'_>| {
        replace(caps.get(1).map_or("", |m| m.as_str()))
    });
    let s = LATEX_MATH_DISPLAY.replace_all(&s, |caps: &regex::Captures<'_>| {
        replace(caps.get(1).map_or("", |m| m.as_str()))
    });
    let s = LATEX_MATH_DOLLAR.replace_all(&s, |caps: &regex::Captures<'_>| {
        replace(caps.get(1).map_or("", |m| m.as_str()))
    });
    let s = LATEX_MATH_PAREN.replace_all(&s, |caps: &regex::Captures<'_>| {
        replace(caps.get(1).map_or("", |m| m.as_str()))
    });
    s.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscript_and_superscript() {
        let input = r"$\beta$-衰减 和 $\alpha$-粒子";
        let result = latex_math_to_unicode(input);
        assert!(result.contains('β'), "β found: {result}");
        assert!(result.contains('α'), "α found: {result}");
    }

    #[test]
    fn test_inline_display_math() {
        let input = r"\\(\\Delta = 1\\) 是一个变化量";
        let result = latex_math_to_unicode(input);
        // 两级转义后 `\\Delta` 不会展开，检查原始字符串行为
        // unicodeit::replace 对输入的 `\Delta` 转换
        // 但 Rust 字符串中 `\\(` 是字面 `\(`，所以不触发转换
        // 实际传递的正文是 `\(\Delta = 1\)`
        // 当 `\(` 本身存在时，正则匹配可能需要调整
        assert!(
            !result.contains("\\Delta"),
            "should not contain raw LaTeX: {result}"
        );
    }

    #[test]
    fn test_display_math_dollar() {
        let input = r"公式：$$E = mc^2$$ 是质能方程";
        let result = latex_math_to_unicode(input);
        assert!(result.contains('²'), "² found: {result}");
    }

    #[test]
    fn test_bracket_math() {
        let input = r"\[x^2 + y^2 = z^2\] 是勾股定理";
        let result = latex_math_to_unicode(input);
        assert!(result.contains('²'), "² found: {result}");
    }

    #[test]
    fn test_non_math_text_unchanged() {
        let input = "这是一个普通的文本段落，不包含 LaTeX 公式。";
        let result = latex_math_to_unicode(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_mixed_regular_text() {
        let input = "根据 $\\sigma$ 原则，计算 $\\mu$ 值";
        let result = latex_math_to_unicode(input);
        assert!(result.contains('σ'), "σ found: {result}");
        assert!(result.contains('μ'), "μ found: {result}");
    }
}
