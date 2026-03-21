//! TUI 焦点、模式与主状态；SGR 鼠标泄漏过滤。

use crate::types::Message;
use ratatui::text::Line;
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

/// 大模型当前阶段（状态栏「状态」字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum ModelPhase {
    /// 空闲，本轮已结束或未开始
    #[default]
    Idle,
    /// 已发起请求，尚未收到流式正文
    Thinking,
    /// 模型正在流式输出 tool_calls（选工具 / 解析参数）
    SelectingTools,
    /// 正在接收模型输出
    Answering,
    /// 正在执行工具
    ToolRunning,
    /// 等待用户审批 shell 命令
    AwaitingApproval,
    /// 流式或协议报错（错误正文不写入对话区，仅状态栏提示）
    Error,
}

impl ModelPhase {
    pub(super) fn label(self) -> &'static str {
        match self {
            ModelPhase::Idle => "就绪",
            ModelPhase::Thinking => "思考中",
            ModelPhase::SelectingTools => "选用工具",
            ModelPhase::Answering => "回答中",
            ModelPhase::ToolRunning => "工具执行中",
            ModelPhase::AwaitingApproval => "等待审批",
            ModelPhase::Error => "异常",
        }
    }
}

/// 单条非 system 消息的聊天行片段（不含消息之间的空行）。
#[derive(Clone)]
pub(super) struct ChatMessageLineCacheEntry {
    pub content_fingerprint: u64,
    pub draw: Vec<Line<'static>>,
    pub plain: Vec<String>,
}

/// 按 `messages` 下标缓存 Markdown 展开结果；宽度或主题变化时整表作废。
#[derive(Clone, Default)]
pub(super) struct ChatLineBuildCache {
    pub chat_inner_width: usize,
    pub md_style: u8,
    pub high_contrast: bool,
    pub code_theme_idx: usize,
    pub per_message: Vec<Option<ChatMessageLineCacheEntry>>,
}

pub(super) struct TuiState {
    // chat
    pub messages: Vec<Message>,
    pub input: String,
    /// `input` 内光标（字节偏移，UTF-8 字符边界）。
    pub input_cursor: usize,
    pub prompt: String,
    pub prompt_cursor: usize,
    pub prompt_title: String,
    pub pending_command: String,
    pub pending_command_args: String,
    /// 与 `command_approval_request.allowlist_key` 对齐；`http_fetch` 永久允许时写入该键而非仅 `pending_command`。
    pub pending_approval_allowlist_key: Option<String>,
    pub approve_choice: u8, // 0=拒绝 1=本次允许 2=永久允许
    pub persistent_command_allowlist: HashSet<String>,
    pub allowlist_file: std::path::PathBuf,
    // runtime
    pub status_line: String,
    pub model_phase: ModelPhase,
    pub tool_running: bool,
    /// 收到 `ToolRunning(false)` 后延迟到本帧 `draw` 之后再清状态，避免与 `true` 在同一轮 `try_recv` 里被冲掉导致状态栏从不显示「工具运行中」。
    pub tool_running_clear_pending: bool,
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
    /// 与 `GET /health` 同逻辑的运行状况（F10）。
    pub show_health: bool,
    pub health_text: String,
    // input area height (in terminal rows)
    pub input_rows: u16,
    pub input_dragging: bool,
    pub input_drag_row: u16,
    /// 聊天区首行在「完整行列表」中的索引（与 `chat_follow_tail` 配合，避免用 offset-from-bottom 在流式重排时闪屏）。
    pub chat_first_line: usize,
    /// 为 true 时每帧将视口钉在最新内容底部（流式输出）；为 false 时保持 `chat_first_line` 只看历史。
    pub chat_follow_tail: bool,
    /// 聊天区逻辑行索引（与 `draw::build_chat_scroll_lines` 纯文本列一致），用于 Ctrl+F 搜索后 F6 切换。
    pub chat_search_matches: Vec<usize>,
    pub chat_search_active_idx: usize,
    pub pending_focus: Option<Focus>,
    pub pending_tab: Option<RightTab>,
    pub mouse_leak_scratch: String,
    /// 聊天输入撤销栈（快照为 `(文本, 光标字节)`）。
    pub input_undo: Vec<(String, usize)>,
    pub input_redo: Vec<(String, usize)>,
    pub prompt_undo: Vec<(String, usize)>,
    pub prompt_redo: Vec<(String, usize)>,
    /// 聊天区 Markdown 按消息缓存（见 `draw::build_chat_scroll_lines`）。
    pub chat_line_build_cache: ChatLineBuildCache,
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
pub(super) fn feed_char_filter_sgr_mouse_leak<F: FnMut(char)>(
    scratch: &mut String,
    ch: char,
    mut push: F,
) {
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

/// 与 [`feed_char_filter_sgr_mouse_leak`] 等价，但将应写入输入框的字符收集为 `Vec`（便于调用方在写入前做撤销检查点）。
pub(super) fn collect_feed_chars_after_sgr_filter(scratch: &mut String, ch: char) -> Vec<char> {
    let mut out = Vec::new();
    feed_char_filter_sgr_mouse_leak(scratch, ch, |c| out.push(c));
    out
}
