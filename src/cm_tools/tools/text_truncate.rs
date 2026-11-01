//! 行级截断（`code_nav` / `symbol` 共用；#1239：两份字节级相同的实现已合并为一份）。
//!
//! 与 [`crate::cm_types::text_utils::preview_chars`] **不是**同一件事：那边是日志预览
//! 后缀 `…(truncated)`，这里是回给模型的工具输出，后缀为 `... (截断)`。语义不同，勿合并。

/// 截断到 `max_chars` 个 Unicode 标量；超出时追加 `... (截断)`。
pub(super) fn truncate_line(s: &str, max_chars: usize) -> String {
    let s = s.trim_end();
    let mut chars = s.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        if let Some(ch) = chars.next() {
            out.push(ch);
        } else {
            return out;
        }
    }
    if s.chars().count() > max_chars {
        format!("{out}... (截断)")
    } else {
        out
    }
}
