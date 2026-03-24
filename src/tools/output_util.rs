//! 工具输出截断公共函数。
//!
//! 多数工具在执行外部命令后需要对 stdout/stderr 做行数 + 字节数双重截断，
//! 避免超长输出占满上下文窗口。此模块统一实现，消除各工具文件中的重复 helper。

/// UTF-8 安全的字节截断：在 `max_bytes` 以内找到最近的 char boundary 并截取。
pub(super) fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// 行数 + 字节数双重截断（UTF-8 安全）。
///
/// 先按 `max_lines` 裁行，再按 `max_bytes` 裁字节。若发生截断则追加摘要后缀。
pub(super) fn truncate_output_lines(s: &str, max_bytes: usize, max_lines: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= max_lines && s.len() <= max_bytes {
        return s.to_string();
    }
    let kept_lines = lines.len().min(max_lines);
    let joined = lines[..kept_lines].join("\n");
    let truncated = if joined.len() <= max_bytes {
        joined
    } else {
        truncate_to_char_boundary(&joined, max_bytes)
    };
    format!(
        "{}\n\n... (输出已截断，保留前 {} 行，共 {} 行)",
        truncated,
        kept_lines,
        lines.len()
    )
}

/// 纯字节截断（UTF-8 安全），适用于不需要按行裁剪的场景（如 diff、结构化数据）。
pub(super) fn truncate_output_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let truncated = truncate_to_char_boundary(s, max_bytes);
    format!(
        "{}\n\n[输出已截断：共 {} 字节，上限 {} 字节]",
        truncated,
        s.len(),
        max_bytes
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_truncation_when_within_limits() {
        let s = "line1\nline2\nline3";
        assert_eq!(truncate_output_lines(s, 1000, 10), s);
    }

    #[test]
    fn truncate_by_line_count() {
        let s = "a\nb\nc\nd\ne";
        let out = truncate_output_lines(s, 10000, 3);
        assert!(out.starts_with("a\nb\nc\n"));
        assert!(out.contains("保留前 3 行"));
        assert!(out.contains("共 5 行"));
    }

    #[test]
    fn truncate_by_byte_limit() {
        let s = "x".repeat(200);
        let out = truncate_output_lines(&s, 50, 1000);
        assert!(out.contains("输出已截断"));
        assert!(out.len() < 200);
    }

    #[test]
    fn char_boundary_safety() {
        let s = "你好世界测试";
        let out = truncate_to_char_boundary(s, 7);
        assert!(out.len() <= 7);
        assert!(out == "你好");
    }

    #[test]
    fn truncate_bytes_only() {
        let s = "a".repeat(200);
        let out = truncate_output_bytes(&s, 50);
        assert!(out.contains("输出已截断"));
        assert!(out.contains("共 200 字节"));
    }

    #[test]
    fn truncate_bytes_no_op() {
        let s = "short";
        assert_eq!(truncate_output_bytes(s, 1000), s);
    }
}
