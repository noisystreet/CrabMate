//! 与配置合并相关的短字符串工具（按 Unicode 标量值截断）。

pub(crate) fn truncate_str_to_max_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
