//! 工具结果写入 UI 存储的 `(compact, detail)` 单入口。

use crate::ToolCardInput;
use crate::ToolCardLocale;
use crate::card::{tool_card_compact_text, tool_card_text};
use crate::parse::parse_tool_envelope;

const COMPACT_MAX_CHARS: usize = 180;

/// SSE `on_tool_result` 与水合 `role=tool` 落盘时的 `(text, reasoning_text)` 形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStoredText {
    pub compact: String,
    pub detail: String,
}

fn compact_one_line(s: &str) -> String {
    let compact = s
        .split_whitespace()
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.chars().count() <= COMPACT_MAX_CHARS {
        return compact;
    }
    let mut out = String::new();
    for ch in compact.chars().take(COMPACT_MAX_CHARS.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

/// 由 [`ToolCardInput`] 生成 compact + detail。
#[must_use]
pub fn tool_stored_text(input: &ToolCardInput, loc: ToolCardLocale) -> ToolStoredText {
    let detail = tool_card_text(input, loc);
    let compact = compact_one_line(&tool_card_compact_text(input, loc));
    ToolStoredText { compact, detail }
}

/// 由 API `role=tool` 信封 JSON 生成展示字段。
pub fn tool_stored_text_from_envelope(
    raw: &str,
    fallback_name: Option<&str>,
    loc: ToolCardLocale,
) -> Option<ToolStoredText> {
    let input = parse_tool_envelope(raw, fallback_name)?;
    Some(tool_stored_text(&input, loc))
}

#[cfg(test)]
mod hydrate_tool_card_golden {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::ToolCardLocale;

    #[test]
    fn hydrate_tool_card_golden() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        let path = root.join("fixtures/hydrate_tool_card_golden.jsonl");
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (line_no, line) in raw.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let mut parts = t.splitn(4, '\t');
            let label = parts.next().unwrap_or("?");
            let envelope = parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing envelope", line_no + 1));
            let compact_needles = parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing compact needles", line_no + 1));
            let detail_needles = parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing detail needles", line_no + 1));
            let loc = ToolCardLocale::ZhHans;
            let sse = tool_stored_text_from_envelope(envelope, None, loc)
                .unwrap_or_else(|| panic!("line {} ({}): parse", line_no + 1, label));
            let input = crate::parse_tool_envelope(envelope, None)
                .unwrap_or_else(|| panic!("line {} ({}): input", line_no + 1, label));
            let direct = tool_stored_text(&input, loc);
            assert_eq!(
                sse,
                direct,
                "line {} ({}): envelope vs input",
                line_no + 1,
                label
            );
            for needle in compact_needles
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                assert!(
                    sse.compact.contains(needle),
                    "line {} ({}): compact {:?} missing {:?}",
                    line_no + 1,
                    label,
                    sse.compact,
                    needle
                );
            }
            for needle in detail_needles
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                assert!(
                    sse.detail.contains(needle),
                    "line {} ({}): detail {:?} missing {:?}",
                    line_no + 1,
                    label,
                    sse.detail,
                    needle
                );
            }
            assert!(!sse.compact.contains("crabmate_tool"));
            assert!(!sse.detail.contains("crabmate_tool"));
        }
    }
}
