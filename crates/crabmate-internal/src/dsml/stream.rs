//! 流式正文增量：在 SSE / CLI / TUI 下发前抑制 DSML 标记，避免 UI 短暂泄露。
//!
//! [`StreamingDsmlContentFilter`] 仅影响**展示路径**；网关原文仍由 `content_acc` 完整累积，供回合结束物化。

use super::normalizer::normalize_deepseek_dsml_vendor_variants;
use super::strip::strip_deepseek_dsml_for_display;

const MAX_INCOMPLETE_DSML_FRAGMENT: usize = 350;

const DSML_OPEN_MARKERS: &[&str] = &[
    "<｜｜DSML｜｜",
    "<||DSML||",
    "<｜DSML｜",
    "<|DSML|",
    "</｜｜DSML｜｜",
    "</||DSML||",
    "</｜DSML｜",
    "</|DSML|",
];

/// 流式抑制 DeepSeek DSML 标记；回合结束物化仍使用未过滤的累积正文。
pub struct StreamingDsmlContentFilter {
    enabled: bool,
    raw_acc: String,
    emitted_stripped_len: usize,
}

impl StreamingDsmlContentFilter {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            raw_acc: String::new(),
            emitted_stripped_len: 0,
        }
    }

    /// 追加网关增量并返回可下发的展示增量（已剥离 DSML）。
    pub fn push_chunk(&mut self, chunk: &str) -> String {
        if !self.enabled || chunk.is_empty() {
            return chunk.to_string();
        }
        self.raw_acc.push_str(chunk);
        self.emit_delta(false)
    }

    /// 流结束时刷出 holdback 尾部的展示增量。
    pub fn finish(&mut self) -> String {
        if !self.enabled {
            return String::new();
        }
        self.emit_delta(true)
    }

    fn emit_delta(&mut self, flush_all: bool) -> String {
        let holdback = if flush_all {
            0
        } else {
            incomplete_dsml_holdback_suffix_len(&self.raw_acc)
        };
        let safe_end = self.raw_acc.len().saturating_sub(holdback);
        let stripped = strip_deepseek_dsml_for_display(&self.raw_acc[..safe_end]);
        self.delta_from_stripped(&stripped)
    }

    fn delta_from_stripped(&mut self, stripped: &str) -> String {
        if stripped.len() <= self.emitted_stripped_len {
            return String::new();
        }
        let delta = stripped[self.emitted_stripped_len..].to_string();
        self.emitted_stripped_len = stripped.len();
        delta
    }
}

fn incomplete_dsml_holdback_suffix_len(raw: &str) -> usize {
    let check = raw.len().min(MAX_INCOMPLETE_DSML_FRAGMENT);
    if check == 0 {
        return 0;
    }
    let tail = &raw[raw.len() - check..];
    let mut best = 0usize;

    for marker in DSML_OPEN_MARKERS {
        let chars: Vec<char> = marker.chars().collect();
        for i in 1..chars.len() {
            let pref: String = chars[..i].iter().collect();
            if tail.ends_with(&pref) {
                best = best.max(pref.len());
            }
        }
    }

    if let Some(rel) = tail.rfind('<') {
        let frag = &tail[rel..];
        if fragment_may_be_incomplete_dsml(frag) {
            best = best.max(frag.len());
        }
    }

    best
}

fn fragment_may_be_incomplete_dsml(frag: &str) -> bool {
    if frag.is_empty() {
        return false;
    }
    if matches!(frag, "<" | "</") {
        return true;
    }
    if looks_like_short_angle_prefix(frag) {
        return true;
    }
    looks_like_unclosed_dsml_tag(frag)
}

fn looks_like_short_angle_prefix(frag: &str) -> bool {
    frag.len() < 9
        && (frag.starts_with('<') || frag.starts_with("</"))
        && !frag.contains('｜')
        && !frag.contains('|')
        && !frag.to_ascii_lowercase().contains("dsml")
}

fn looks_like_unclosed_dsml_tag(frag: &str) -> bool {
    let norm = normalize_deepseek_dsml_vendor_variants(frag);
    let lower = norm.to_ascii_lowercase();
    if !(lower.contains("dsml") || lower.contains("<|") || frag.contains('｜')) {
        return false;
    }
    if let Some(open) = norm.rfind("<|DSML|").or_else(|| norm.rfind("<｜DSML｜"))
        && !norm[open..].contains('>')
    {
        return true;
    }
    norm.contains("DSML") && !frag.contains('>')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_chunks(chunks: &[&str]) -> String {
        let mut f = StreamingDsmlContentFilter::new(true);
        let mut out = String::new();
        for c in chunks {
            out.push_str(&f.push_chunk(c));
        }
        out.push_str(&f.finish());
        out
    }

    #[test]
    fn stream_matches_batch_strip_invoke_split() {
        let full = "说明。\n<｜DSML｜tool_calls>\n<｜DSML｜invoke name=\"run_command\">\n</｜DSML｜invoke>\n</｜DSML｜tool_calls>\n尾部";
        let chunks = [
            "说明。\n<｜DS",
            "ML｜tool_calls>\n<｜DSML｜invoke name=\"run_command\">\n</｜DSML｜",
            "invoke>\n</｜DSML｜tool_calls>\n尾部",
        ];
        assert_eq!(
            stream_chunks(&chunks),
            strip_deepseek_dsml_for_display(full)
        );
    }

    #[test]
    fn stream_matches_batch_strip_double_pipe() {
        let full = "前言<｜｜DSML｜｜invoke name=\"f\"></｜｜DSML｜｜invoke>后记";
        let chunks = [
            "前言<｜｜DS",
            "ML｜｜invoke name=\"f\"></｜｜DSML｜｜invoke>后记",
        ];
        assert_eq!(
            stream_chunks(&chunks),
            strip_deepseek_dsml_for_display(full)
        );
    }

    #[test]
    fn disabled_filter_passthrough() {
        let mut f = StreamingDsmlContentFilter::new(false);
        assert_eq!(f.push_chunk("<｜DSML｜invoke>"), "<｜DSML｜invoke>");
        assert_eq!(f.finish(), "");
    }

    #[test]
    fn plain_text_unchanged() {
        let chunks = ["hello ", "world"];
        assert_eq!(stream_chunks(&chunks), "hello world");
    }
}
