//! 标签字节扫描与「最早匹配区间」合并（`think` / `redacted_thinking`）。

/// Plain 与行内代码形态（反引号包裹）的 redacted_thinking 开闭标签。
pub(super) const REDACTED_LIKE_PAIRS: &[(&str, &str)] = &[
    (
        concat!("<", "redacted", "_", "thinking", ">"),
        concat!("</", "redacted", "_", "thinking", ">"),
    ),
    (
        concat!("`", "<", "redacted", "_", "thinking", ">", "`"),
        concat!("`", "</", "redacted", "_", "thinking", ">", "`"),
    ),
];

/// `<` 或 `</` 之后、大小写不敏感的 `redacted_thinking>`（ASCII）。
pub(super) const REDACTED_TAG_INNER_ASCII_LOWER: &[u8] = b"redacted_thinking>";

/// Qwen / vLLM 等使用的 think 开闭标签（plain 与反引号包裹）。
pub(super) const THINK_LIKE_PAIRS: &[(&str, &str)] = &[
    (concat!("<", "think", ">"), concat!("</", "think", ">")),
    (
        concat!("`", "<", "think", ">", "`"),
        concat!("`", "</", "think", ">", "`"),
    ),
];

/// `<` 或 `</` 之后、大小写不敏感的 `think>`（ASCII）。
pub(super) const THINK_TAG_INNER_ASCII_LOWER: &[u8] = b"think>";

#[inline]
pub(super) fn merge_earlier_span_candidate(
    best: Option<(usize, usize)>,
    cand: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    let Some((s, e)) = cand else {
        return best;
    };
    match best {
        None => Some((s, e)),
        Some((bs, _be)) if s < bs => Some((s, e)),
        Some((bs, be)) if s == bs && e > be => Some((s, e)),
        o => o,
    }
}

/// 在字面量 `pairs` 上找最早出现的开标签或闭标签（由 `pick_close` 选择）。
pub(super) fn fold_literal_pair_spans(
    rest: &str,
    pairs: &[(&str, &str)],
    pick_close: bool,
) -> Option<(usize, usize)> {
    let mut best = None;
    for (open, close) in pairs {
        let needle = if pick_close { *close } else { *open };
        if let Some(rel) = rest.find(needle) {
            let end = rel + needle.len();
            best = merge_earlier_span_candidate(best, Some((rel, end)));
        }
    }
    best
}

pub(super) fn find_earliest_open_span(
    rest: &str,
    pairs: &[(&str, &str)],
    plain_open: fn(&str) -> Option<(usize, usize)>,
    bt_open: fn(&str) -> Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    let mut best = fold_literal_pair_spans(rest, pairs, false);
    best = merge_earlier_span_candidate(best, plain_open(rest));
    merge_earlier_span_candidate(best, bt_open(rest))
}

pub(super) fn find_earliest_close_span(
    rest: &str,
    pairs: &[(&str, &str)],
    plain_close: fn(&str) -> Option<(usize, usize)>,
    bt_close: fn(&str) -> Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    let mut best = fold_literal_pair_spans(rest, pairs, true);
    best = merge_earlier_span_candidate(best, plain_close(rest));
    merge_earlier_span_candidate(best, bt_close(rest))
}

pub(super) fn bytes_slice_ci_eq_lower(hay: &[u8], i: usize, lower_ascii: &[u8]) -> bool {
    if i + lower_ascii.len() > hay.len() {
        return false;
    }
    for (k, &lb) in lower_ascii.iter().enumerate() {
        if hay[i + k].to_ascii_lowercase() != lb {
            return false;
        }
    }
    true
}

pub(super) fn find_ci_plain_redacted_close_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i + 2 + REDACTED_TAG_INNER_ASCII_LOWER.len() <= b.len() {
        if b[i] == b'<'
            && b[i + 1] == b'/'
            && bytes_slice_ci_eq_lower(b, i + 2, REDACTED_TAG_INNER_ASCII_LOWER)
        {
            let end = i + 2 + REDACTED_TAG_INNER_ASCII_LOWER.len();
            return Some((i, end));
        }
        i += 1;
    }
    None
}

pub(super) fn find_ci_backtick_redacted_open_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let inner_len = REDACTED_TAG_INNER_ASCII_LOWER.len();
    let mut i = 0usize;
    while i + 2 + inner_len < b.len() {
        if b[i] == b'`'
            && b[i + 1] == b'<'
            && bytes_slice_ci_eq_lower(b, i + 2, REDACTED_TAG_INNER_ASCII_LOWER)
            && b[i + 2 + inner_len] == b'`'
        {
            return Some((i, i + 2 + inner_len + 1));
        }
        i += 1;
    }
    None
}

pub(super) fn find_ci_backtick_redacted_close_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let inner_len = REDACTED_TAG_INNER_ASCII_LOWER.len();
    let mut i = 0usize;
    while i + 3 + inner_len < b.len() {
        if b[i] == b'`'
            && b[i + 1] == b'<'
            && b[i + 2] == b'/'
            && bytes_slice_ci_eq_lower(b, i + 3, REDACTED_TAG_INNER_ASCII_LOWER)
            && b[i + 3 + inner_len] == b'`'
        {
            return Some((i, i + 3 + inner_len + 1));
        }
        i += 1;
    }
    None
}

pub(super) fn find_ci_plain_redacted_open_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i + 1 + REDACTED_TAG_INNER_ASCII_LOWER.len() <= b.len() {
        if b[i] == b'<' && bytes_slice_ci_eq_lower(b, i + 1, REDACTED_TAG_INNER_ASCII_LOWER) {
            let end = i + 1 + REDACTED_TAG_INNER_ASCII_LOWER.len();
            return Some((i, end));
        }
        i += 1;
    }
    None
}

pub(super) fn find_earliest_redacted_open_span(rest: &str) -> Option<(usize, usize)> {
    find_earliest_open_span(
        rest,
        REDACTED_LIKE_PAIRS,
        find_ci_plain_redacted_open_span,
        find_ci_backtick_redacted_open_span,
    )
}

pub(super) fn find_earliest_redacted_close_span(rest: &str) -> Option<(usize, usize)> {
    find_earliest_close_span(
        rest,
        REDACTED_LIKE_PAIRS,
        find_ci_plain_redacted_close_span,
        find_ci_backtick_redacted_close_span,
    )
}

pub(super) fn find_ci_plain_think_close_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i + 2 + THINK_TAG_INNER_ASCII_LOWER.len() <= b.len() {
        if b[i] == b'<'
            && b[i + 1] == b'/'
            && bytes_slice_ci_eq_lower(b, i + 2, THINK_TAG_INNER_ASCII_LOWER)
        {
            let end = i + 2 + THINK_TAG_INNER_ASCII_LOWER.len();
            return Some((i, end));
        }
        i += 1;
    }
    None
}

pub(super) fn find_ci_backtick_think_open_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let inner_len = THINK_TAG_INNER_ASCII_LOWER.len();
    let mut i = 0usize;
    while i + 2 + inner_len < b.len() {
        if b[i] == b'`'
            && b[i + 1] == b'<'
            && bytes_slice_ci_eq_lower(b, i + 2, THINK_TAG_INNER_ASCII_LOWER)
            && b[i + 2 + inner_len] == b'`'
        {
            return Some((i, i + 2 + inner_len + 1));
        }
        i += 1;
    }
    None
}

pub(super) fn find_ci_backtick_think_close_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let inner_len = THINK_TAG_INNER_ASCII_LOWER.len();
    let mut i = 0usize;
    while i + 3 + inner_len < b.len() {
        if b[i] == b'`'
            && b[i + 1] == b'<'
            && b[i + 2] == b'/'
            && bytes_slice_ci_eq_lower(b, i + 3, THINK_TAG_INNER_ASCII_LOWER)
            && b[i + 3 + inner_len] == b'`'
        {
            return Some((i, i + 3 + inner_len + 1));
        }
        i += 1;
    }
    None
}

pub(super) fn find_ci_plain_think_open_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i + 1 + THINK_TAG_INNER_ASCII_LOWER.len() <= b.len() {
        if b[i] == b'<' && bytes_slice_ci_eq_lower(b, i + 1, THINK_TAG_INNER_ASCII_LOWER) {
            let end = i + 1 + THINK_TAG_INNER_ASCII_LOWER.len();
            return Some((i, end));
        }
        i += 1;
    }
    None
}

pub(super) fn find_earliest_think_open_span(rest: &str) -> Option<(usize, usize)> {
    find_earliest_open_span(
        rest,
        THINK_LIKE_PAIRS,
        find_ci_plain_think_open_span,
        find_ci_backtick_think_open_span,
    )
}

pub(super) fn find_earliest_think_close_span(rest: &str) -> Option<(usize, usize)> {
    find_earliest_close_span(
        rest,
        THINK_LIKE_PAIRS,
        find_ci_plain_think_close_span,
        find_ci_backtick_think_close_span,
    )
}

pub(super) fn strip_trailing_partial_open_generic<'a>(
    s: &'a str,
    streaming: bool,
    pairs: &[(&str, &str)],
    tag_inner_lower: &[u8],
) -> &'a str {
    if !streaming || s.is_empty() {
        return s;
    }
    let b = s.as_bytes();
    let mut longest = 0usize;
    for (open, _) in pairs {
        let ob = open.as_bytes();
        for k in 1..=ob.len().min(b.len()) {
            if b[b.len() - k..] == ob[..k] {
                longest = longest.max(k);
            }
        }
    }
    for k in 1..=(1 + tag_inner_lower.len()).min(b.len()) {
        let start = b.len() - k;
        if b[start] != b'<' {
            continue;
        }
        let after_lt = start + 1;
        let inner_len = b.len() - after_lt;
        if inner_len == 0 {
            longest = longest.max(k);
            continue;
        }
        if bytes_slice_ci_eq_lower(b, after_lt, &tag_inner_lower[..inner_len]) {
            longest = longest.max(k);
        }
    }
    if longest > 0 {
        &s[..s.len() - longest]
    } else {
        s
    }
}
