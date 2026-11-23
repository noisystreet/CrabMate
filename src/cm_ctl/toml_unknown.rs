//! 从声明 / 预设文件文本里**结构化**找出未知键名。
//!
//! `toml::de::Error` 只暴露 `span()`，给不出字段名，于是 `instances.rs` / `profiles.rs`
//! 曾用 `detail.contains("unknown field `workspace`")` 反推用户提示——toml 升级或引号风格
//! 一变（`` `workspace` `` vs `'workspace'`），提示就静默消失。这里改成在 `toml::Value`
//! 上比对调用方给出的已知键集合，**判定不再读任何错误文案**。

/// `text` 里 `table` 段名下第一个条目中出现的未知键名。
///
/// 只看条目级未知键：本助手专门用来解释「合法键写错位置」（`workspace` / `yes` 被写进
/// profile），顶层未知表不影响这些提示。`known` 由各模块提供，并用「填满字段的样例」
/// 用例钉住，避免与结构体漂移。
pub(crate) fn first_unknown_entry_key(
    text: &str,
    table: &str,
    known: &[&str],
) -> Option<String> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let entries = value.as_table()?.get(table)?.as_table()?;
    for (_, entry) in entries {
        let Some(entry) = entry.as_table() else {
            continue;
        };
        if let Some(key) = entry.keys().find(|k| !known.contains(&k.as_str())) {
            return Some(key.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::first_unknown_entry_key;

    const KNOWN: &[&str] = &["known_key"];

    #[test]
    fn finds_entry_level_unknown_key_without_parsing_messages() {
        assert_eq!(
            first_unknown_entry_key("[items.a]\nknown_key = \"x\"\ntypo = 1\n", "items", KNOWN),
            Some("typo".to_string())
        );
        assert_eq!(
            first_unknown_entry_key("[items.a]\nknown_key = \"x\"\n", "items", KNOWN),
            None
        );
        // 顶层未知表不属于条目级未知键。
        assert_eq!(
            first_unknown_entry_key("[other]\nx = 1\n", "items", KNOWN),
            None
        );
        // 非法 TOML 时不猜键名。
        assert_eq!(first_unknown_entry_key("[[[ bad", "items", KNOWN), None);
    }
}
