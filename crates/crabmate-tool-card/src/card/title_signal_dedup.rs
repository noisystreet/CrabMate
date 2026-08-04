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

/// 紧凑条右侧「信号」与左侧标题仅下划线/空格差异时视为同一信息（如 `git_status` 与 `git status`），不拼分隔符。
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

/// 去掉紧凑串开头的工具 id / 标题及常见分隔符（空格、`·`、`｜`）。
fn strip_leading_tool_title<'a>(title: &str, compact: &'a str) -> &'a str {
    let t = title.trim();
    let s = compact.trim();
    if t.is_empty() {
        return s;
    }
    let Some(rest) = s.strip_prefix(t) else {
        return s;
    };
    rest.trim_start_matches(|c: char| c.is_whitespace() || c == '·' || c == '｜' || c == '|')
        .trim()
}

/// 标题栏已展示工具名时，按候选标题剥掉 compact 前缀后的旁侧信号。
pub(super) fn tool_signal_beside_titles(titles: &[&str], compact: &str) -> Option<String> {
    let compact = compact.trim();
    if compact.is_empty() {
        return None;
    }
    let mut signal = compact.to_string();
    loop {
        let before = signal.clone();
        for t in titles {
            let t = t.trim();
            if t.is_empty() {
                continue;
            }
            signal = strip_leading_tool_title(t, &signal).to_string();
        }
        if signal == before {
            break;
        }
    }
    if signal.is_empty() {
        return None;
    }
    for t in titles {
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(tail) = tool_compact_signal_paren_suffix_after_redundant_head(t, &signal) {
            // `(exit=N)` 是结果元数据，不是 git `(working)` 这类模式后缀；保留完整 CLI 信号。
            if tail.starts_with("(exit=") {
                continue;
            }
            return Some(tail);
        }
        if tool_compact_signal_redundant_with_title(t, &signal) {
            // `cargo check (exit=101)` 与 `cargo_check` 头同义，但不能整段丢掉。
            if signal.contains("(exit=") {
                continue;
            }
            return None;
        }
    }
    Some(signal)
}

/// 标题栏已展示工具名时，旁侧应显示的信号（去掉同义标题；保留 `(working)` 等后缀）。
///
/// 整段与标题同义、或去掉标题后为空 → [`None`]（调用方勿再回退成工具名，以免 `git_diff_stat 完成 git_diff_stat`）。
#[must_use]
pub fn tool_signal_beside_title(title: &str, compact: &str) -> Option<String> {
    tool_signal_beside_titles(&[title], compact)
}
