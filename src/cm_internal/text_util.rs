//! 与 UI/日志相关的短字符串工具（按 Unicode 标量值截断）。

/// 取前 `max` 个字符；若原文更长则在末尾加 `…`（用于错误摘要等）。
pub fn truncate_chars_with_ellipsis(s: &str, max: usize) -> String {
    let t: String = s.chars().take(max).collect();
    if t.len() < s.len() {
        format!("{t}…")
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsis_when_longer() {
        assert_eq!(truncate_chars_with_ellipsis("abcdef", 3), "abc…");
        assert_eq!(truncate_chars_with_ellipsis("ab", 3), "ab");
    }
}
