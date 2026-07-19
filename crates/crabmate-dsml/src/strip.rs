//! 从用户可见正文中剥除 DeepSeek DSML 标记。

use regex::Regex;
use std::sync::LazyLock;

use crate::normalizer::normalize_deepseek_dsml_vendor_variants;
use crate::strip_scan::{
    collapse_blank_runs, strip_dsml_named_blocks_ascii, strip_dsml_named_blocks_fullwidth,
};

static STRIP_DSML_ORDERED_BLOCK_RES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    const PATTERNS: &[&str] = &[
        r"(?s)<｜DSML｜tool_calls\b[^>]*>.*?</｜DSML｜tool_calls>",
        r"(?s)<\|DSML\|tool_calls\b[^>]*>.*?</\|DSML\|tool_calls>",
        r"(?s)<｜DSML｜parameter\b[^>]*>.*?</｜DSML｜parameter>",
        r"(?s)<｜DSML｜invoke\b[^>]*>.*?</｜DSML｜invoke>",
        r"(?s)<｜DSML｜function_calls\b[^>]*>.*?</｜DSML｜function_calls>",
        r"(?s)<\|DSML\|parameter\b[^>]*>.*?</\|DSML\|parameter>",
        r"(?s)<\|DSML\|invoke\b[^>]*>.*?</\|DSML\|invoke>",
        r"(?s)<\|DSML\|function_calls\b[^>]*>.*?</\|DSML\|function_calls>",
    ];
    PATTERNS
        .iter()
        .map(|p| Regex::new(p).expect("strip_deepseek_dsml: static DSML block regex must compile"))
        .collect()
});

static STRIP_DSML_ORPHAN_OPEN_FW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<｜DSML｜[^>\n]{0,300}>")
        .expect("strip_deepseek_dsml: orphan fullwidth open-tag regex must compile")
});
static STRIP_DSML_ORPHAN_CLOSE_FW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"</｜DSML｜[^>\n]{1,80}>")
        .expect("strip_deepseek_dsml: orphan fullwidth close-tag regex must compile")
});
static STRIP_DSML_ORPHAN_OPEN_ASCII: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<\|DSML\|[^>\n]{0,300}>")
        .expect("strip_deepseek_dsml: orphan ASCII open-tag regex must compile")
});
static STRIP_DSML_ORPHAN_CLOSE_ASCII: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"</\|DSML\|[^>\n]{1,80}>")
        .expect("strip_deepseek_dsml: orphan ASCII close-tag regex must compile")
});

/// 去掉 DeepSeek **DSML** 结构化片段，避免规划与对话区出现标记而非自然语言。
pub fn strip_deepseek_dsml_for_display(s: &str) -> String {
    let s = normalize_deepseek_dsml_vendor_variants(s);
    if !s.contains("DSML") {
        return s;
    }
    let mut out = s;

    for re in STRIP_DSML_ORDERED_BLOCK_RES.iter() {
        loop {
            let next = re.replace_all(&out, "").to_string();
            if next == out {
                break;
            }
            out = next;
        }
    }

    out = strip_dsml_named_blocks_fullwidth(&out);
    out = strip_dsml_named_blocks_ascii(&out);

    out = STRIP_DSML_ORPHAN_OPEN_FW.replace_all(&out, "").to_string();
    out = STRIP_DSML_ORPHAN_CLOSE_FW.replace_all(&out, "").to_string();
    out = STRIP_DSML_ORPHAN_OPEN_ASCII
        .replace_all(&out, "")
        .to_string();
    out = STRIP_DSML_ORPHAN_CLOSE_ASCII
        .replace_all(&out, "")
        .to_string();

    collapse_blank_runs(&out)
}
