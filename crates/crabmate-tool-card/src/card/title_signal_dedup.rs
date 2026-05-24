//! 紧凑条标题与摘要「信号」同义判定（snake_case / CLI / 括号后缀）。

/// 将工具 id / CLI 写法规范为可比较的 token 串（`_`、`-`、空白视为等价分隔）。
fn normalize_tool_label_for_dedup(s: &str) -> String {
    let mapped: String = s
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| match c {
            '_' | '-' => ' ',
            c => c,
        })
        .collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 紧凑条右侧「信号」与左侧标题仅下划线/空格差异时视为同一信息（如 `git_status` 与 `git status`），不拼 `｜`。
#[inline]
fn compact_title_signal_redundant(title: &str, signal: &str) -> bool {
    let t = title.trim();
    let s = signal.trim();
    if s.is_empty() {
        return true;
    }
    if s == t {
        return true;
    }
    if t.replace('_', " ").eq_ignore_ascii_case(s) || s.replace(' ', "_").eq_ignore_ascii_case(t) {
        return true;
    }
    normalize_tool_label_for_dedup(t) == normalize_tool_label_for_dedup(s)
}

/// 紧凑「信号」整段或其 **`(` 前** 的学名人部分是否与标题同义（如 `git_diff` vs `git diff (working): …`）。
pub(super) fn tool_compact_signal_redundant_with_title(title: &str, signal: &str) -> bool {
    if compact_title_signal_redundant(title, signal) {
        return true;
    }
    let head = signal
        .trim()
        .split_once('(')
        .map(|(before, _)| before.trim())
        .filter(|h| !h.is_empty());
    let Some(h) = head else {
        return false;
    };
    compact_title_signal_redundant(title, h)
}

/// `git_diff` 与 `git diff (working): …` 同义时，返回 **`(` 起** 的后缀（保留参数与工作区提示）；整段已同义则 `None`。
pub(super) fn tool_compact_signal_paren_suffix_after_redundant_head(
    title: &str,
    signal: &str,
) -> Option<String> {
    let s = signal.trim();
    if s.is_empty() || compact_title_signal_redundant(title, s) {
        return None;
    }
    let head = s
        .split_once('(')
        .map(|(before, _)| before.trim())
        .filter(|h| !h.is_empty())?;
    if !compact_title_signal_redundant(title, head) {
        return None;
    }
    let i = s.find('(')?;
    let tail = s[i..].trim_start();
    (!tail.is_empty()).then(|| tail.to_string())
}
