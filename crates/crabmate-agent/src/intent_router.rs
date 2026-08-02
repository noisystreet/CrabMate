//! 多轮「确认执行」识别（无意图分类）。

/// 助手等待用户确认执行时的参考文案（多轮识别用）。
pub const EXECUTE_CONFIRM: &str =
    "我判断你可能想让我直接执行任务。请确认是否“直接开始执行”，或补充更具体范围。";

const EXPLICIT_EXECUTE_CONFIRM_KEYWORDS: &[&str] = &[
    "直接开始执行",
    "开始执行",
    "直接执行",
    "确认执行",
    "继续执行",
    "现在执行",
    "马上执行",
];

pub fn is_explicit_execute_confirmation(s: &str) -> bool {
    EXPLICIT_EXECUTE_CONFIRM_KEYWORDS
        .iter()
        .any(|k| s.contains(k))
}

/// 助手是否正在等待用户确认执行（供多轮上下文复用）。
pub fn is_waiting_execute_confirmation_prompt(assistant_text: &str) -> bool {
    let t = assistant_text.trim();
    let t_lower = t.to_lowercase();
    !t.is_empty()
        && (t == EXECUTE_CONFIRM
            || t.contains("请确认是否“直接开始执行”")
            || (t.contains("请确认是否") && (t.contains("开始执行") || t.contains("直接执行")))
            || (t_lower.contains("confirm")
                && (t_lower.contains("execute") || t_lower.contains("run"))))
}

#[cfg(test)]
mod tests {
    use super::{
        EXECUTE_CONFIRM, is_explicit_execute_confirmation, is_waiting_execute_confirmation_prompt,
    };

    #[test]
    fn waiting_confirm_prompt_matches_canned_and_paraphrase() {
        assert!(is_waiting_execute_confirmation_prompt(EXECUTE_CONFIRM));
        assert!(is_waiting_execute_confirmation_prompt(
            "请确认是否开始执行上述改动。"
        ));
        assert!(!is_waiting_execute_confirmation_prompt("随便聊聊"));
    }

    #[test]
    fn explicit_confirm_keywords() {
        assert!(is_explicit_execute_confirmation("直接开始执行"));
        assert!(is_explicit_execute_confirmation("请继续执行吧"));
        assert!(!is_explicit_execute_confirmation("继续看看文档"));
    }
}
