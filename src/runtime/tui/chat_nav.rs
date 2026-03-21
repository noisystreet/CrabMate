//! 聊天区搜索与按消息跳转（逻辑行与 `draw::build_chat_scroll_lines` 的纯文本列一致）。

use super::draw::{build_chat_scroll_lines, chat_inner_width_from_term_cols};
use super::state::TuiState;

pub(super) const PROMPT_TITLE_SEARCH: &str = "搜索聊天";
pub(super) const PROMPT_TITLE_JUMP: &str = "跳转消息序号";

fn truncate_status_query(s: &str) -> String {
    let mut t = s.to_string();
    if t.chars().count() > 20 {
        t = t.chars().take(20).collect::<String>();
        t.push('…');
    }
    t
}

pub(super) fn apply_chat_search(state: &mut TuiState, query: &str, term_cols: u16) {
    let q = query.trim();
    if q.is_empty() {
        state.chat_search_matches.clear();
        state.chat_search_active_idx = 0;
        state.status_line = "搜索词为空".to_string();
        return;
    }
    let w = chat_inner_width_from_term_cols(term_cols);
    let (_, plain_lines, _) = build_chat_scroll_lines(state, w);
    let low = q.to_lowercase();
    let matches: Vec<usize> = plain_lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains(&low))
        .map(|(i, _)| i)
        .collect();
    if matches.is_empty() {
        state.chat_search_matches.clear();
        state.chat_search_active_idx = 0;
        state.status_line = format!("未找到「{}」", truncate_status_query(q));
        return;
    }
    state.chat_search_matches = matches;
    state.chat_search_active_idx = 0;
    jump_to_search_match(state);
    state.status_line = format!("找到 {} 处", state.chat_search_matches.len());
}

pub(super) fn jump_to_search_match(state: &mut TuiState) {
    if state.chat_search_matches.is_empty() {
        return;
    }
    let i = state
        .chat_search_active_idx
        .min(state.chat_search_matches.len() - 1);
    let line = state.chat_search_matches[i];
    state.chat_follow_tail = false;
    state.chat_first_line = line;
}

pub(super) fn search_next(state: &mut TuiState, dir: i32) {
    if state.chat_search_matches.is_empty() {
        return;
    }
    let len = state.chat_search_matches.len();
    let cur = state.chat_search_active_idx;
    let next = if dir > 0 {
        (cur + 1) % len
    } else {
        (cur + len - 1) % len
    };
    state.chat_search_active_idx = next;
    jump_to_search_match(state);
    state.status_line = format!("搜索结果 {}/{}", state.chat_search_active_idx + 1, len);
}

/// `n` 为从 1 起的「非 system」可见消息序号（与聊天区展示顺序一致）。
pub(super) fn apply_jump_to_message(state: &mut TuiState, input: &str, term_cols: u16) -> bool {
    let n: usize = match input.trim().parse::<usize>() {
        Ok(v) if v >= 1 => v,
        _ => {
            state.status_line = "请输入 ≥1 的整数（不含系统提示的可见消息编号）".to_string();
            return false;
        }
    };
    let w = chat_inner_width_from_term_cols(term_cols);
    let (_, _, starts) = build_chat_scroll_lines(state, w);
    if n > starts.len() {
        state.status_line = format!("仅有 {} 条可见消息", starts.len());
        return false;
    }
    state.chat_search_matches.clear();
    state.chat_search_active_idx = 0;
    state.chat_follow_tail = false;
    state.chat_first_line = starts[n - 1];
    state.status_line = format!("已跳到第 {} 条消息", n);
    true
}
