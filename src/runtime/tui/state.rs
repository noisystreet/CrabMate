//! TUI 焦点、模式与主状态；SGR 鼠标泄漏过滤。

use crate::types::Message;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    ChatView,
    ChatInput,
    Workspace,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Normal,
    FileView,
    Prompt,
    CommandApprove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RightTab {
    Workspace = 0,
    Tasks = 1,
    Schedule = 2,
}

impl RightTab {
    pub(super) fn titles() -> [&'static str; 3] {
        ["工作区", "任务", "日程"]
    }
}

pub(super) fn focus_name(f: Focus) -> &'static str {
    match f {
        Focus::ChatView => "聊天区",
        Focus::ChatInput => "输入区",
        Focus::Workspace => "工作区",
        Focus::Right => "右侧面板",
    }
}

pub(super) struct TuiState {
    // chat
    pub messages: Vec<Message>,
    pub input: String,
    pub prompt: String,
    pub prompt_title: String,
    pub pending_command: String,
    pub pending_command_args: String,
    pub approve_choice: u8, // 0=拒绝 1=本次允许 2=永久允许
    pub persistent_command_allowlist: HashSet<String>,
    pub allowlist_file: std::path::PathBuf,
    // runtime
    pub status_line: String,
    pub tool_running: bool,
    pub focus: Focus,
    pub mode: Mode,
    // right panel
    pub tab: RightTab,
    // workspace view
    pub workspace_dir: std::path::PathBuf,
    pub workspace_entries: Vec<(String, bool)>, // (name, is_dir)
    pub workspace_sel: usize,
    // file view
    pub file_view_title: String,
    pub file_view_content: String,
    // tasks view
    pub task_items: Vec<(String, String, bool)>, // (id,title,done)
    pub task_sel: usize,
    // schedule view (reminders)
    pub reminder_items: Vec<(String, String, bool, Option<String>)>, // (id,title,done,due_at)
    pub reminder_sel: usize,
    // schedule view (events)
    pub event_items: Vec<(String, String, String)>, // (id,title,start_at)
    pub event_sel: usize,
    pub schedule_sub: u8, // 0=reminders, 1=events
    // markdown rendering
    pub md_style: u8, // 0=dark, 1=light
    pub high_contrast: bool,
    pub code_theme_idx: usize,
    // help overlay
    pub show_help: bool,
    // input area height (in terminal rows)
    pub input_rows: u16,
    pub input_dragging: bool,
    pub input_drag_row: u16,
    // chat scroll offset (0 = bottom, >0 = scrolled up)
    pub chat_scroll: i32,
    pub cursor_override: Option<(u16, u16)>,
    pub pending_focus: Option<Focus>,
    pub pending_tab: Option<RightTab>,
    pub cursor_mouse_pos: Option<(u16, u16)>,
    pub mouse_leak_scratch: String,
}

/// xterm SGR 鼠标报告：`\x1b[<btn;col;rowM`；若 CSI 被吞掉，可见部分形如 `<35;50;30M`。
static SGR_MOUSE_LEAK_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^<\d+;\d+;\d+[Mm]$").unwrap_or_else(|e| {
        tracing::warn!(error = %e, "SGR_MOUSE_LEAK_TAIL regex invalid; using no-match fallback");
        Regex::new("a^").expect("infallible empty-match regex")
    })
});

static SGR_MOUSE_LEAK_EMBEDDED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[<\d+;\d+;\d+[Mm]|<\d+;\d+;\d+[Mm]").unwrap_or_else(|e| {
        tracing::warn!(error = %e, "SGR_MOUSE_LEAK_EMBEDDED regex invalid; using no-match fallback");
        Regex::new("a^").expect("infallible empty-match regex")
    })
});

pub(super) fn strip_sgr_mouse_leaks(s: &str) -> String {
    SGR_MOUSE_LEAK_EMBEDDED.replace_all(s, "").into_owned()
}

/// 丢弃误送入的 SGR 鼠标片段；否则将 `scratch` 与当前字符按用户输入写入 `push`。
pub(super) fn feed_char_filter_sgr_mouse_leak<F: FnMut(char)>(scratch: &mut String, ch: char, mut push: F) {
    const MAX: usize = 32;
    if scratch.len() >= MAX {
        let old = std::mem::take(scratch);
        for c in old.chars() {
            push(c);
        }
    }
    if scratch.is_empty() {
        if ch == '<' {
            scratch.push(ch);
        } else {
            push(ch);
        }
        return;
    }
    let valid_next = ch.is_ascii_digit() || ch == ';' || ch == 'M' || ch == 'm';
    if !valid_next {
        let old = std::mem::take(scratch);
        for c in old.chars() {
            push(c);
        }
        push(ch);
        return;
    }
    scratch.push(ch);
    if ch == 'M' || ch == 'm' {
        if SGR_MOUSE_LEAK_TAIL.is_match(scratch.as_str()) {
            scratch.clear();
        } else {
            let old = std::mem::take(scratch);
            for c in old.chars() {
                push(c);
            }
        }
    }
}
