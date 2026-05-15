//! 时间线/气泡展示用纯文本拼装（无 `ChatStreamCallbackCtx` 副作用）。

use crate::i18n;

pub(crate) fn non_empty_trimmed_tool_name(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

pub(crate) fn build_final_response_text(title: &str, detail: Option<&str>) -> String {
    let mut final_text = title.trim().to_string();
    if let Some(detail) = detail.map(str::trim)
        && !detail.is_empty()
    {
        if !final_text.is_empty() {
            final_text.push_str("\n\n");
        }
        final_text.push_str(detail);
    }
    final_text
}

pub(crate) fn build_intent_analysis_main_bubble_text(title: &str, detail: Option<&str>) -> String {
    let title = title.trim();
    let detail = detail.map(str::trim).unwrap_or("");
    let mut out = String::new();
    if !title.is_empty() {
        out.push_str(title);
    }
    if !detail.is_empty() {
        let mut confidence = String::new();
        let mut primary = String::new();
        let mut clarification = String::new();
        let mut l2 = String::new();
        for line in detail.lines().map(str::trim) {
            match i18n::classify_intent_detail_line(line) {
                Some(i18n::IntentDetailLineKind::Confidence) => confidence = line.to_string(),
                Some(i18n::IntentDetailLineKind::PrimaryIntent) => primary = line.to_string(),
                Some(i18n::IntentDetailLineKind::NeedClarification) => {
                    clarification = line.to_string();
                }
                Some(i18n::IntentDetailLineKind::L2Result) => l2 = line.to_string(),
                None => {}
            }
        }
        let concise = [confidence, primary, clarification, l2]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !concise.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&concise);
        }
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{out}\n\n")
    }
}

pub(crate) fn build_hierarchical_plan_main_bubble_text(
    title: &str,
    detail: Option<&str>,
) -> String {
    let mut out = String::new();
    let title = title.trim();
    if !title.is_empty() {
        out.push_str(title);
    }
    if let Some(detail) = detail.map(str::trim)
        && !detail.is_empty()
    {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(detail);
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{out}\n\n")
    }
}

pub(crate) fn build_hierarchical_subgoal_main_bubble_text(
    title: &str,
    detail: Option<&str>,
) -> String {
    let mut out = title.trim().to_string();
    if let Some(detail) = detail.map(str::trim)
        && !detail.is_empty()
    {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(detail);
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{out}\n\n")
    }
}

pub(crate) fn to_single_line(s: &str, max_chars: usize) -> String {
    let compact = s
        .split_whitespace()
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let mut out = String::new();
    for ch in compact.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}
