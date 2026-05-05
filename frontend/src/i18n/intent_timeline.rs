//! 意图分析时间线 `detail` 行前缀（`format_intent_detail` 等）；中英双语解析。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentDetailLineKind {
    Confidence,
    PrimaryIntent,
    NeedClarification,
    L2Result,
}

/// 将 `detail` 中的单行归类为意图摘要块关心的键行（忽略其它观测行）。
pub fn classify_intent_detail_line(line: &str) -> Option<IntentDetailLineKind> {
    let t = line.trim();
    if t.starts_with("综合置信度：") || t.starts_with("Overall confidence:") {
        Some(IntentDetailLineKind::Confidence)
    } else if t.starts_with("主意图：") || t.starts_with("Primary intent:") {
        Some(IntentDetailLineKind::PrimaryIntent)
    } else if t.starts_with("需要澄清：") || t.starts_with("Needs clarification:") {
        Some(IntentDetailLineKind::NeedClarification)
    } else if t.starts_with("L2 结果：")
        || t.starts_with("L2 result:")
        || t.starts_with("L2 Result:")
    {
        Some(IntentDetailLineKind::L2Result)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_en_intent_lines() {
        assert_eq!(
            classify_intent_detail_line("Primary intent: x"),
            Some(IntentDetailLineKind::PrimaryIntent)
        );
        assert_eq!(
            classify_intent_detail_line("Overall confidence: 0.5"),
            Some(IntentDetailLineKind::Confidence)
        );
        assert_eq!(
            classify_intent_detail_line("L2 result: ok"),
            Some(IntentDetailLineKind::L2Result)
        );
        assert_eq!(
            classify_intent_detail_line("Needs clarification: false"),
            Some(IntentDetailLineKind::NeedClarification)
        );
    }
}
