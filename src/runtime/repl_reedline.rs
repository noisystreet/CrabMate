//! REPL 行读取与 **reedline** 集成：历史、Emacs 编辑键、多行指示；TTY 专用。
//!
//! **TTY**：**缓冲区为空**时按 **`$`** 或全角 **`＄`**（可带 **Shift**，视终端而定）**立即**切换「我:」与 **`bash#:`**，无需先按 Enter；仍兼容**单独一行 `$` 后 Enter**。**stdin** 为 TTY 即使用 **reedline**（勿再要求 **stdout** 为 TTY）。管道读行与编辑器共用 **`shell_mode`**。

use std::borrow::Cow;
use std::cell::Cell;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use reedline::{
    Color as ReedlineColor, EditMode, Emacs, FileBackedHistory, Prompt, PromptEditMode,
    PromptHistorySearch, PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineRawEvent,
    Signal, default_emacs_keybindings,
};

use super::cli_repl_ui::{CLI_PROMPT_AFTER_COLON, cli_repl_stdout_use_color};

const REPL_HISTORY_MAX: usize = 4096;
const MULTILINE_INDICATOR: &str = "::: ";

thread_local! {
    static REEDLINE_BUFFER_PROBE: Cell<Option<*const Reedline>> = const { Cell::new(None) };
}

/// 在单次 [`Reedline::read_line`] 调用期间挂上指针，供 [`DollarToggleEmacs`] 用 [`Reedline::current_buffer_contents`] 判断是否空行。
///
/// # Safety
/// `reedline` 必须在整个 `read_line` 调用期间保持有效；仅在同一线程、嵌套于该 `read_line` 栈内通过 `current_buffer_contents` **只读**使用。
struct ReedlineReadLineScope;

impl ReedlineReadLineScope {
    unsafe fn arm(reedline: *const Reedline) -> Self {
        REEDLINE_BUFFER_PROBE.set(Some(reedline));
        Self
    }
}

impl Drop for ReedlineReadLineScope {
    fn drop(&mut self) {
        REEDLINE_BUFFER_PROBE.set(None);
    }
}

fn reedline_current_buffer_is_empty() -> bool {
    REEDLINE_BUFFER_PROBE.with(|c| {
        c.get()
            .is_some_and(|p| unsafe { (*p).current_buffer_contents().is_empty() })
    })
}

fn key_is_plain_dollar(code: &KeyCode, modifiers: KeyModifiers) -> bool {
    let ch = match code {
        KeyCode::Char(c) => *c,
        _ => return false,
    };
    if ch != '$' && ch != '\u{ff04}' {
        return false;
    }
    let blocked =
        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER;
    !modifiers.intersects(blocked)
}

/// 缓冲区为空时按 `$` / `＄` 立即 [`ReedlineEvent::ExecuteHostCommand`]（虚拟行 `"$"`），其余键委托 **Emacs**。
struct DollarToggleEmacs {
    emacs: Emacs,
}

impl DollarToggleEmacs {
    fn new() -> Self {
        Self {
            emacs: Emacs::new(default_emacs_keybindings()),
        }
    }
}

impl EditMode for DollarToggleEmacs {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        let ev = Event::from(event);
        if let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = &ev
            && key_is_plain_dollar(code, *modifiers)
            && reedline_current_buffer_is_empty()
        {
            return ReedlineEvent::ExecuteHostCommand("$".to_string());
        }
        match ReedlineRawEvent::try_from(ev) {
            Ok(raw) => self.emacs.parse_event(raw),
            Err(()) => ReedlineEvent::None,
        }
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.emacs.edit_mode()
    }
}

/// 行首 **`$`** 或全角 **`＄`**：`Some(None)` 表示仅美元符一行（切换 **我:/bash#:**）；`Some(Some(cmd))` 为待执行的 shell 一行；`None` 表示非美元行。
pub(crate) fn parse_repl_dollar_shell_line(input: &str) -> Option<Option<&str>> {
    let t = input.trim_start();
    let after = if let Some(r) = t.strip_prefix('$') {
        r
    } else if let Some(r) = t.strip_prefix('\u{ff04}') {
        r
    } else {
        return None;
    };
    let rest = after.trim();
    if rest.is_empty() {
        Some(None)
    } else {
        Some(Some(rest))
    }
}

/// REPL 单次读取结果（在 `spawn_blocking` 内完成）。
#[derive(Debug)]
pub(crate) enum ReplReadLine {
    /// stdin EOF（如 Ctrl+D）
    Eof,
    /// 空行（忽略，继续下一轮）
    Empty,
    /// 普通对话文本
    Chat(String),
    /// 本地 shell：`None` 为应打印用法；`Some` 为命令行（TTY 下不含 `$` 前缀）
    Shell(Option<String>),
}

/// 非 **reedline** 读行：与 [`ReplLineEditor`] 共用 **`shell_mode`**，裸 **`$`/`＄`** 行为与 TTY 一致（切换而非打印 shell 用法）。
pub(crate) fn read_repl_line_piped(shell_mode: &Arc<AtomicBool>) -> io::Result<ReplReadLine> {
    let mut input = String::new();
    let n = io::stdin().lock().read_line(&mut input)?;
    if n == 0 {
        return Ok(ReplReadLine::Eof);
    }
    let line = input.trim_end_matches(['\r', '\n']);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        if shell_mode.load(Ordering::Relaxed) {
            return Ok(ReplReadLine::Shell(None));
        }
        return Ok(ReplReadLine::Empty);
    }
    if let Some(opt) = parse_repl_dollar_shell_line(trimmed) {
        match opt {
            None => {
                shell_mode.fetch_xor(true, Ordering::SeqCst);
                return Ok(ReplReadLine::Empty);
            }
            Some(cmd) => {
                return Ok(ReplReadLine::Shell(Some(cmd.to_string())));
            }
        }
    }
    if shell_mode.load(Ordering::Relaxed) {
        return Ok(ReplReadLine::Shell(Some(trimmed.to_string())));
    }
    Ok(ReplReadLine::Chat(trimmed.to_string()))
}

#[derive(Clone)]
struct CrabmatePrompt {
    shell_mode: Arc<AtomicBool>,
    ansi_prompt: bool,
}

fn format_prompt_left_string(shell: bool, ansi_prompt: bool) -> io::Result<String> {
    if !ansi_prompt {
        return Ok(if shell {
            format!("bash#:{CLI_PROMPT_AFTER_COLON}")
        } else {
            format!("我:{CLI_PROMPT_AFTER_COLON}")
        });
    }
    let mut buf = Vec::new();
    if shell {
        crate::runtime::terminal_labels::write_repl_bash_prompt_prefix(&mut buf)?;
    } else {
        crate::runtime::terminal_labels::write_user_message_prefix(&mut buf)?;
    }
    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

impl Prompt for CrabmatePrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        let shell = self.shell_mode.load(Ordering::Relaxed);
        match format_prompt_left_string(shell, self.ansi_prompt) {
            Ok(s) => Cow::Owned(s),
            Err(_) => Cow::Owned(if shell {
                format!("bash#:{CLI_PROMPT_AFTER_COLON}")
            } else {
                format!("我:{CLI_PROMPT_AFTER_COLON}")
            }),
        }
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _edit_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed(MULTILINE_INDICATOR)
    }

    fn render_prompt_history_search_indicator(
        &self,
        prompt_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match prompt_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!(
            "({}reverse-search: {}) ",
            prefix, prompt_search.term
        ))
    }

    fn get_prompt_color(&self) -> ReedlineColor {
        ReedlineColor::Reset
    }

    fn get_indicator_color(&self) -> ReedlineColor {
        ReedlineColor::Reset
    }

    fn get_prompt_right_color(&self) -> ReedlineColor {
        ReedlineColor::Reset
    }
}

/// 持有 **reedline** 状态；须在 **spawn_blocking** 中调用 [`Self::read_line`]。
pub(crate) struct ReplLineEditor {
    reedline: Reedline,
    shell_mode: Arc<AtomicBool>,
    prompt: CrabmatePrompt,
}

impl ReplLineEditor {
    /// `history_file`：通常为 `{run_command_working_dir}/.crabmate/repl_history.txt`。
    pub fn new(history_file: &Path) -> io::Result<Self> {
        let ansi_prompt = cli_repl_stdout_use_color();
        let shell_mode = Arc::new(AtomicBool::new(false));
        let prompt = CrabmatePrompt {
            shell_mode: shell_mode.clone(),
            ansi_prompt,
        };

        let history = FileBackedHistory::with_file(REPL_HISTORY_MAX, history_file.to_path_buf())
            .map_err(|e| io::Error::other(e.to_string()))?;

        let reedline = Reedline::create()
            .with_history(Box::new(history))
            .with_ansi_colors(ansi_prompt)
            .with_edit_mode(Box::new(DollarToggleEmacs::new()));

        Ok(Self {
            reedline,
            shell_mode,
            prompt,
        })
    }

    pub(crate) fn shell_mode_arc(&self) -> &Arc<AtomicBool> {
        &self.shell_mode
    }

    pub fn read_line(&mut self) -> io::Result<ReplReadLine> {
        loop {
            let sig = {
                let _probe = unsafe { ReedlineReadLineScope::arm(&self.reedline as *const _) };
                self.reedline
                    .read_line(&self.prompt)
                    .map_err(|e| io::Error::other(e.to_string()))?
            };
            match sig {
                Signal::Success(text) => {
                    let t = text.trim_end_matches(['\r', '\n']);
                    if let Some(opt) = parse_repl_dollar_shell_line(t) {
                        match opt {
                            None => {
                                self.shell_mode.fetch_xor(true, Ordering::SeqCst);
                                continue;
                            }
                            Some(cmd) if self.shell_mode.load(Ordering::Relaxed) => {
                                return Ok(ReplReadLine::Shell(Some(cmd.to_string())));
                            }
                            Some(_) => {}
                        }
                    }
                    if self.shell_mode.load(Ordering::Relaxed) {
                        if t.trim().is_empty() {
                            return Ok(ReplReadLine::Shell(None));
                        }
                        return Ok(ReplReadLine::Shell(Some(t.to_string())));
                    }
                    if t.trim().is_empty() {
                        return Ok(ReplReadLine::Empty);
                    }
                    return Ok(ReplReadLine::Chat(text));
                }
                Signal::CtrlD => return Ok(ReplReadLine::Eof),
                Signal::CtrlC => continue,
            }
        }
    }
}

/// TTY 用 **reedline**；否则走管道读行。
pub(crate) fn read_repl_line_with_editor(editor: &mut ReplLineEditor) -> io::Result<ReplReadLine> {
    let mut stdout = io::stdout();
    let _ = stdout.flush();
    // 仅依赖 stdin：避免 `cargo run … >log` 等 stdout 非 TTY 时误用管道读行，导致裸 `$` 变成 Shell(None) 刷屏用法。
    if io::stdin().is_terminal() {
        editor.read_line()
    } else {
        read_repl_line_piped(editor.shell_mode_arc())
    }
}
