//! Rust 单文件大纲：`collect_outline` 的正则预编译与逐行匹配。

use regex::Regex;

pub(super) fn collect_outline(
    content: &str,
    include_use: bool,
    max_items: usize,
) -> Vec<(usize, String)> {
    let patterns = OutlinePatterns::new();
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if out.len() >= max_items {
            break;
        }
        let line_no = idx + 1;
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("//!") {
            continue;
        }
        if let Some(summary) = patterns.match_line(line, include_use) {
            out.push((line_no, summary));
        }
    }
    out
}

struct OutlinePatterns {
    re_mod: Regex,
    re_fn: Regex,
    re_struct: Regex,
    re_enum: Regex,
    re_trait: Regex,
    re_type: Regex,
    re_const: Regex,
    re_static: Regex,
    re_macro: Regex,
    re_impl: Regex,
    re_use: Regex,
}

impl OutlinePatterns {
    fn new() -> Self {
        Self {
            re_mod: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\b").expect("valid regex"),
            re_fn: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)\b")
                .expect("valid regex"),
            re_struct: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+(\w+)\b")
                .expect("valid regex"),
            re_enum: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+(\w+)\b")
                .expect("valid regex"),
            re_trait: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?trait\s+(\w+)\b")
                .expect("valid regex"),
            re_type: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?type\s+(\w+)\b")
                .expect("valid regex"),
            re_const: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+(\w+)\b")
                .expect("valid regex"),
            re_static: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?static\s+(\w+)\b")
                .expect("valid regex"),
            re_macro: Regex::new(r"^\s*macro_rules!\s+(\w+)\b").expect("valid regex"),
            re_impl: Regex::new(r"^\s*(?:unsafe\s+)?impl\b").expect("valid regex"),
            re_use: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+").expect("valid regex"),
        }
    }

    fn match_line(&self, line: &str, include_use: bool) -> Option<String> {
        self.try_mod(line)
            .or_else(|| self.try_fn(line))
            .or_else(|| self.try_struct(line))
            .or_else(|| self.try_enum(line))
            .or_else(|| self.try_trait(line))
            .or_else(|| self.try_type(line))
            .or_else(|| self.try_const(line))
            .or_else(|| self.try_static(line))
            .or_else(|| self.try_macro(line))
            .or_else(|| self.try_impl(line))
            .or_else(|| self.try_use(line, include_use))
    }

    fn try_mod(&self, line: &str) -> Option<String> {
        let c = self.re_mod.captures(line)?;
        Some(format!("mod {}", &c[1]))
    }

    fn try_fn(&self, line: &str) -> Option<String> {
        self.re_fn
            .is_match(line)
            .then(|| truncate_one_line(line, 100))
    }

    fn try_struct(&self, line: &str) -> Option<String> {
        let c = self.re_struct.captures(line)?;
        Some(format!("struct {}", &c[1]))
    }

    fn try_enum(&self, line: &str) -> Option<String> {
        let c = self.re_enum.captures(line)?;
        Some(format!("enum {}", &c[1]))
    }

    fn try_trait(&self, line: &str) -> Option<String> {
        let c = self.re_trait.captures(line)?;
        Some(format!("trait {}", &c[1]))
    }

    fn try_type(&self, line: &str) -> Option<String> {
        self.re_type
            .is_match(line)
            .then(|| truncate_one_line(line, 100))
    }

    fn try_const(&self, line: &str) -> Option<String> {
        let c = self.re_const.captures(line)?;
        Some(format!("const {}", &c[1]))
    }

    fn try_static(&self, line: &str) -> Option<String> {
        let c = self.re_static.captures(line)?;
        Some(format!("static {}", &c[1]))
    }

    fn try_macro(&self, line: &str) -> Option<String> {
        let c = self.re_macro.captures(line)?;
        Some(format!("macro_rules! {}", &c[1]))
    }

    fn try_impl(&self, line: &str) -> Option<String> {
        self.re_impl
            .is_match(line)
            .then(|| truncate_one_line(line, 120))
    }

    fn try_use(&self, line: &str, include_use: bool) -> Option<String> {
        if !include_use {
            return None;
        }
        self.re_use
            .is_match(line)
            .then(|| truncate_one_line(line, 100))
    }
}

fn truncate_one_line(s: &str, max: usize) -> String {
    let t = s.trim_end();
    let mut out: String = t.chars().take(max).collect();
    if t.chars().count() > max {
        out.push('…');
    }
    out
}
