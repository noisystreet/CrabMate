//! 文本工具函数（从 `crabmate-tools::redact` 提取）。
//!
//! 供 `crabmate-memory` 等轻量 crate 使用，无需引入完整工具依赖。

/// 按 Unicode 标量截断；超出则后缀 `…(truncated)`。
pub fn preview_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut iter = s.chars();
    let prefix: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{prefix}…(truncated)")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::preview_chars;

    #[test]
    fn short_string_not_truncated() {
        assert_eq!(preview_chars("hello", 10), "hello");
    }

    #[test]
    fn long_string_truncated() {
        let out = preview_chars("hello world", 5);
        assert!(out.starts_with("hello"));
        assert!(out.contains("truncated"));
    }

    #[test]
    fn zero_max_returns_empty() {
        assert_eq!(preview_chars("any", 0), "");
    }

    #[test]
    fn handles_unicode() {
        let s = "你好世界";
        assert_eq!(preview_chars(s, 2), "你好…(truncated)");
        assert_eq!(preview_chars(s, 4), "你好世界");
    }
}
