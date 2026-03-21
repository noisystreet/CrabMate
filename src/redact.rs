//! 将**上游 HTTP 响应体**等长文本截断为适合 **tracing** 的预览，避免把全文写入日志。
//!
//! 与仓库「密钥与日志脱敏」规则配合：**不得**在 `error!` / 返回给前端的 `Err` 中附带完整供应商响应体；
//! 排障时使用 `body_preview` + `body_len` 即可。

/// 日志里展示的响应体预览最大字符数（Unicode 标量）。
pub const HTTP_BODY_PREVIEW_LOG_CHARS: usize = 256;

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

/// 将空白（含换行、制表）规范为**单空格**后截断，便于结构化日志单行输出。
pub fn single_line_preview(s: &str, max_chars: usize) -> String {
    let folded = s.split_whitespace().collect::<Vec<_>>().join(" ");
    preview_chars(&folded, max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_truncates_with_marker() {
        let s = "a".repeat(10);
        assert_eq!(preview_chars(&s, 5), "aaaaa…(truncated)");
        assert_eq!(preview_chars("hi", 10), "hi");
    }

    #[test]
    fn single_line_collapses_newlines() {
        assert_eq!(single_line_preview("a\nb\r\nc", 20), "a b c");
        assert_eq!(single_line_preview("  x  \t y  ", 20), "x y");
    }
}
