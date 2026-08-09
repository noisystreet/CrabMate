//! TUI 共享模型：焦点、布局、撰写区折行与 [`TuiModel`]。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Text};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::text_util::truncate_chars_with_ellipsis;
use crate::tool_approval::TuiApprovalRequest;

use super::approval;
use super::clarify_modal;
use super::transcript;
use super::turn_project;
use super::workspace_modal;

/// 撰写区行首提示符（与 [`composer_wrap_lines`] 起始列一致）。
pub(super) const COMPOSER_PROMPT_PREFIX: &str = "› ";

/// Agent 异步侧与 UI 线程共享：澄清问卷 inbox + 待并入下一条用户消息的答案。
#[derive(Clone)]
pub(crate) struct TuiClarificationShared {
    pub(super) inbox: Arc<Mutex<VecDeque<crate::sse::ClarificationQuestionnaireBody>>>,
    pub(super) answers_merge:
        Arc<Mutex<Option<crate::clarification_questionnaire::ClarifyAnswersNormalized>>>,
}

pub(crate) enum UiEvent {
    Quit,
    Submit(String),
    /// 工作区路径原始输入（由 Modal 或后续扩展提交）。
    WorkspaceSwitch(String),
}

/// 可聚焦面板（鼠标点击 / Tab 切换）；用于边框高亮。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TuiFocus {
    NavLeft,
    Chat,
    #[default]
    Composer,
    SideRight,
}

impl TuiFocus {
    pub(super) fn cycle_next(self) -> Self {
        match self {
            Self::NavLeft => Self::Chat,
            Self::Chat => Self::Composer,
            Self::Composer => Self::SideRight,
            Self::SideRight => Self::NavLeft,
        }
    }

    pub(super) fn cycle_prev(self) -> Self {
        match self {
            Self::NavLeft => Self::SideRight,
            Self::Chat => Self::NavLeft,
            Self::Composer => Self::Chat,
            Self::SideRight => Self::Composer,
        }
    }
}

/// 与 [`render::render_full`] 一致的分区，供鼠标命中与绘制共用。
pub(crate) struct TuiPaneLayout {
    pub(super) nav_left: Rect,
    pub(super) chat: Rect,
    pub(super) composer: Rect,
    pub(super) side_right: Rect,
}

pub(crate) fn compute_tui_pane_layout(area: Rect) -> TuiPaneLayout {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(23),
            Constraint::Percentage(54),
            Constraint::Percentage(23),
        ])
        .split(vertical[1]);

    // 撰写区固定高度，避免随终端拉高占用过多；聊天区吃掉剩余空间。
    let center_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(4)])
        .split(horizontal[1]);

    TuiPaneLayout {
        nav_left: horizontal[0],
        chat: center_chunks[0],
        composer: center_chunks[1],
        side_right: horizontal[2],
    }
}

pub(crate) fn rect_contains(r: Rect, col: u16, row: u16) -> bool {
    let cx = r.x <= col && col < r.x.saturating_add(r.width);
    let cy = r.y <= row && row < r.y.saturating_add(r.height);
    cx && cy
}

pub(crate) fn focus_at_point(layout: &TuiPaneLayout, col: u16, row: u16) -> Option<TuiFocus> {
    if rect_contains(layout.nav_left, col, row) {
        return Some(TuiFocus::NavLeft);
    }
    if rect_contains(layout.chat, col, row) {
        return Some(TuiFocus::Chat);
    }
    if rect_contains(layout.composer, col, row) {
        return Some(TuiFocus::Composer);
    }
    if rect_contains(layout.side_right, col, row) {
        return Some(TuiFocus::SideRight);
    }
    None
}

/// 按显示宽度折行，返回每一行文本及逻辑光标所在行、列（列宽为单元格，`>= max_width` 时表示换行后的「下一行首」）。
fn composer_wrap_lines(max_width: usize, input: &str) -> (Vec<String>, usize, usize) {
    let mut lines = vec![String::from(COMPOSER_PROMPT_PREFIX)];
    let mut row = 0usize;
    let mut col = COMPOSER_PROMPT_PREFIX.width();

    for ch in input.chars() {
        let mut w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w == 0 {
            lines[row].push(ch);
            continue;
        }
        w = w.max(1);
        if col + w > max_width {
            lines.push(String::new());
            row += 1;
            col = 0;
        }
        lines[row].push(ch);
        col += w;
    }

    let mut cur_row = row;
    let mut cur_col = col;
    if cur_col >= max_width {
        cur_row += 1;
        cur_col = 0;
    }
    while lines.len() <= cur_row {
        lines.push(String::new());
    }

    (lines, cur_row, cur_col)
}

/// 生成撰写区可见行（底部对齐滚动）及相对于 inner 左上角的光标坐标。
pub(crate) fn composer_visible_and_cursor_rel(
    inner: Rect,
    input: &str,
) -> (Text<'static>, Option<(u16, u16)>) {
    let mw = inner.width as usize;
    let mh = inner.height as usize;
    if mw == 0 || mh == 0 {
        return (Text::from(Line::from(COMPOSER_PROMPT_PREFIX)), None);
    }
    if COMPOSER_PROMPT_PREFIX.width() > mw {
        let clipped = truncate_chars_with_ellipsis(COMPOSER_PROMPT_PREFIX, mw);
        return (Text::from(Line::from(clipped)), Some((0u16, 0u16)));
    }

    let (lines, cur_row, cur_col) = composer_wrap_lines(mw, input);
    let scroll = lines.len().saturating_sub(mh);
    let visible: Vec<Line<'static>> = lines.into_iter().skip(scroll).map(Line::from).collect();
    let cursor_row = cur_row.saturating_sub(scroll);
    let cy = cursor_row.min(mh.saturating_sub(1));
    let cx = cur_col.min(mw.saturating_sub(1));
    (Text::from(visible), Some((cx as u16, cy as u16)))
}

pub(crate) struct TuiModel {
    /// 顶栏一行：`CrabMate · 工作目录`（模型见底栏 chips）
    pub(super) header_line: String,
    /// 左栏：会话文件、`tui_session.json` 与加载开关等（对齐 Web 左侧会话）
    pub(super) nav_summary: String,
    /// 右栏：当前工作区路径（仅路径；无任务清单 / 变更预览）。
    pub(super) right_summary: String,
    pub(super) transcript: String,
    /// 聊天区垂直滚动（`Paragraph::scroll` 的 y）；须与 [`render::clamped_chat_vertical_scroll`] 一致地 clamp，避免 ratatui `scroll_y` 过大导致溢出 panic。
    /// 流式生成时由 [`render::render_full`] 写回贴底偏移；**勿**用远大于 `max_scroll` 的哨兵值（否则滚轮每次 `-3` 需很久才生效，表现为卡顿）。
    pub(super) chat_scroll_y: u16,
    /// 回合刷新 transcript 后下一帧按当前布局写入真实贴底 `chat_scroll_y`（见 [`render::render_full`]）。
    pub(super) chat_snap_bottom_next_draw: bool,
    /// 与 Web `auto_scroll_chat` 同源：`true` = 流式/增高时贴底；上滑 unpin，近底 / 下滑回阈值 re-pin。
    pub(super) chat_follow_bottom: bool,
    /// 用户主动下滑（滚轮↓ / PgDn）：下一帧按 gap 判定是否 re-pin（对齐 Web `scrolled_down`）。
    pub(super) chat_user_scroll_down: bool,
    /// 左键在聊天区纵向滚动条上按下后拖动（[`super::mouse_key::tui_dispatch_mouse`]）。
    pub(super) chat_scrollbar_dragging: bool,
    pub(super) input: String,
    /// 与 Web 底栏 chips 同源快照（`模型 · … · 角色 · …`），左对齐。
    pub(super) status_chips: String,
    /// 与 Web / Tauri `StatusBarRunIndicator` 同源（就绪 / 模型生成中… / 工具执行中… / 错误），底栏最右。
    pub(super) status_run: String,
    pub(super) focus: TuiFocus,
    /// 敏感工具审批 Modal（单条）；多条时先入队。
    pub(super) approval_modal: Option<approval::TuiApprovalModalState>,
    pub(super) approval_backlog: VecDeque<TuiApprovalRequest>,
    /// 澄清问卷（与 Web SSE `clarification_questionnaire` 对齐）。
    pub(super) clarification_modal: Option<clarify_modal::TuiClarificationModalState>,
    pub(super) clarification_backlog: VecDeque<crate::sse::ClarificationQuestionnaireBody>,
    /// 与异步侧 `work_dir` 同步，供 UI 打开工作区 Modal。
    pub(super) workspace_path_buf: std::path::PathBuf,
    /// 工作区切换（目录浏览 + 手动路径，对齐 Web `POST /workspace` / REPL `/workspace`）。
    pub(super) workspace_modal: Option<workspace_modal::TuiWorkspaceModalState>,
    /// 已启用 **`conversation_store_sqlite_path`** 时当前 **`conversation_id`**（左栏与会话命令同源）。
    pub(super) sqlite_conversation_id: Option<String>,
    /// 最近会话缓存（左栏「最近会话」；有 SQLite 时刷新；含与 Tauri 同源标题）。
    pub(super) recent_conversations: Vec<crate::conversation_store::ConversationListEntry>,
    /// 本轮 SSE 控制面镜像（无 HTTP 通道时与 Web `SsePayload` 对齐）。
    pub(super) control_plane_tail: String,
    /// 本轮 canonical Turn 投影（与 Web/Tauri `project_turn_web_v2` 同行序）。
    pub(super) turn_projection: turn_project::TuiTurnProjection,
    /// 已定稿回合展示（含 flush 后的投影行序，无 `[Turn 投影]` 元标签）；`msg_len` 与 `messages` 前缀对齐。
    pub(super) committed_turns: transcript::CommittedTurns,
}
