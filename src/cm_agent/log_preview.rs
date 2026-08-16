//! 日志预览截断（与根包 `redact::preview_chars` 语义一致；避免 agent 域 crate 依赖 HTTP/编排层）。

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
