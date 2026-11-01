//! 文本工具函数（从 `cm_tools::redact` 提取）。
//!
//! 供 `cm_memory` 等轻量模块使用，无需引入完整工具依赖。

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

/// UTF-8 安全的字节截断：在 `max_bytes` 以内找到最近的 char boundary 并截取。
///
/// 曾在 `cm_tools::tools::output_util`、`cm_memory::…::numeric`、`cm_workflow::execute::report`
/// 各有一份**字节级相同**的副本（后者注释直接写着「复制自 output_util」）；`#1247` 收归此处，
/// 避免改一处漏两处——多字节字符边界处理漂移会直接 panic 或截错。
pub fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::{preview_chars, truncate_to_char_boundary};

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

    #[test]
    fn truncate_respects_utf8_boundary() {
        // 「你」占 3 字节：max=4 落在第二个字中间，必须回退到边界，不能 panic 也不能截半字。
        assert_eq!(truncate_to_char_boundary("你好世界", 4), "你");
        assert_eq!(truncate_to_char_boundary("你好世界", 6), "你好");
        assert_eq!(truncate_to_char_boundary("hello", 3), "hel");
        assert_eq!(truncate_to_char_boundary("hello", 99), "hello");
        assert_eq!(truncate_to_char_boundary("你好", 0), "");
    }
}
