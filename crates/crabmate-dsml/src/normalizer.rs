//! DeepSeek DSML 变体归一化（双竖线、全角竖线、标签间空白等）。

use regex::Regex;
use std::sync::LazyLock;

/// 双竖线 `<｜｜DSML｜｜` 等折叠为单竖线形式。
pub fn normalize_deepseek_dsml_vendor_variants(s: &str) -> String {
    s.replace("<｜｜DSML｜｜", "<｜DSML｜")
        .replace("</｜｜DSML｜｜", "</｜DSML｜")
        .replace("<||DSML||", "<|DSML|")
        .replace("</||DSML||", "</|DSML|")
}

/// 全角 `｜` 与 ASCII `|` 混用的 DSML 统一成 ASCII 尖括号形式，便于正则解析。
pub fn normalize_deepseek_dsml_brackets(s: &str) -> String {
    let s = normalize_deepseek_dsml_vendor_variants(s);
    s.replace("<｜DSML｜", "<|DSML|")
        .replace("</｜DSML｜", "</|DSML|")
}

/// 模型常在 `<`、`|`、`DSML` 之间插空格（如 `< | DSML | invoke`），会导致整段正则匹配失败。
pub fn normalize_deepseek_dsml_tag_spacing(s: &str) -> String {
    static LOOSE_OPEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<\s*\|\s*DSML\s*\|")
            .expect("normalize_dsml_spacing: loose open-tag regex must compile")
    });
    static LOOSE_SHUT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"</\s*\|\s*DSML\s*\|")
            .expect("normalize_dsml_spacing: loose close-prefix regex must compile")
    });
    static COMPRESS_AFTER_OPEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<\|DSML\|\s+")
            .expect("normalize_dsml_spacing: compress-after-open regex must compile")
    });
    static COMPRESS_AFTER_SHUT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"</\|DSML\|\s+")
            .expect("normalize_dsml_spacing: compress-after-shut regex must compile")
    });
    let t = LOOSE_OPEN.replace_all(s, "<|DSML|");
    let t = LOOSE_SHUT.replace_all(&t, "</|DSML|");
    let t = COMPRESS_AFTER_OPEN.replace_all(&t, "<|DSML|");
    COMPRESS_AFTER_SHUT.replace_all(&t, "</|DSML|>").to_string()
}

pub fn normalize_for_parse(s: &str) -> String {
    normalize_deepseek_dsml_tag_spacing(&normalize_deepseek_dsml_brackets(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_double_pipe_collapses() {
        let s = "<｜｜DSML｜｜invoke>";
        assert!(normalize_deepseek_dsml_vendor_variants(s).contains("<｜DSML｜"));
        assert!(!normalize_deepseek_dsml_vendor_variants(s).contains("｜｜"));
    }
}
