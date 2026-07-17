//! 助手正文 fuzzy 比较（normalize 空白后比较）；Phase 7 P1 起写/读路径不再全表 dedupe，本模块供 snapshot 判定与单测保留。

/// 折叠空白便于比较排版略有差异的重复终答/旁注。
#[must_use]
pub fn normalize_assistant_text_for_dedupe(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 两段助手正文是否语义重复（相等，或较长段包含较短段且短段足够长）。
#[must_use]
pub fn assistant_texts_fuzzy_duplicate(a: &str, b: &str) -> bool {
    let a = normalize_assistant_text_for_dedupe(a.trim());
    let b = normalize_assistant_text_for_dedupe(b.trim());
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let (long, short) = if a.len() >= b.len() {
        (a.as_str(), b.as_str())
    } else {
        (b.as_str(), a.as_str())
    };
    short.len() > 40 && long.contains(short)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize_assistant_text_for_dedupe("a\n\nb  c"), "a b c");
    }

    #[test]
    fn fuzzy_duplicate_detects_compact_vs_expanded() {
        let expanded = "当前目录下有三个压缩包：\n\n1. **A** — x\n\n2. **B** — y";
        let compact = "当前目录下有三个压缩包：\n1. **A** — x\n2. **B** — y";
        assert!(assistant_texts_fuzzy_duplicate(expanded, compact));
    }
}
