//! REPL 行读取与 **reedline** 集成：历史、Emacs 编辑键、多行指示、**Tab** 补全内建 **`/`** 命令；TTY 专用。
//!
//! **TTY**：**缓冲区无可见内容**（`trim` 后为空）时按 **`$`** 或全角 **`＄`** **立即**切换「我:」与 **`bash#:`**，无需先按 Enter；仍兼容**单独一行 `$` 后 Enter**。
//! **修饰键**：允许 **Shift**（如美式 **Shift+4**）与 **AltGr**（常见为 **Ctrl+Alt**，欧洲布局输入 `$`）；仍拒绝 **Ctrl+$**、**单 Alt+$** 等。
//! **stdin** 为 TTY 即使用 **reedline**（勿再要求 **stdout** 为 TTY）。管道读行与编辑器共用 **`shell_mode`**。

use std::borrow::Cow;
use std::cell::Cell;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use reedline::{
    Color as ReedlineColor, ColumnarMenu, Completer, EditMode, Emacs, FileBackedHistory,
    Keybindings, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, ReedlineRawEvent, Signal,
    Span, Suggestion, default_emacs_keybindings,
};

use super::cli_repl_ui::{CLI_PROMPT_AFTER_COLON, cli_repl_stdout_use_color};

const REPL_HISTORY_MAX: usize = 4096;
const MULTILINE_INDICATOR: &str = "::: ";

/// reedline 补全菜单名，须与 [`repl_emacs_keybindings`] 内 `ReedlineEvent::Menu` 一致。
const REPL_COMPLETION_MENU: &str = "completion_menu";

/// 内建 `/` 命令名（不含斜杠；`?` 单独成项）。
const SLASH_COMMANDS: &[&str] = &[
    "?",
    "agent",
    "cd",
    "clear",
    "config",
    "doctor",
    "export",
    "help",
    "mcp",
    "model",
    "models",
    "probe",
    "save-session",
    "tools",
    "version",
    "workspace",
];

/// `/export` 与 `/save-session` 后的格式参数（与 REPL `repl_export_kind_from_arg` 一致）。
const EXPORT_FORMAT_ARGS: &[&str] = &["both", "json", "markdown", "md"];

fn repl_emacs_keybindings() -> Keybindings {
    let mut kb = default_emacs_keybindings();
    kb.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu(REPL_COMPLETION_MENU.to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    kb
}

/// 行首（允许前导空白）的 `/` 内建命令补全；**bash#:** 模式下关闭。
struct ReplSlashCompleter {
    shell_mode: Arc<AtomicBool>,
}

impl ReplSlashCompleter {
    fn new(shell_mode: Arc<AtomicBool>) -> Self {
        Self { shell_mode }
    }

    /// `before_cursor` = `line[..pos]`。仅当「去掉前导空白后以 `/` 开头」时返回该 `/` 的字节下标。
    fn slash_command_start_byte(before_cursor: &str) -> Option<usize> {
        let trimmed = before_cursor.trim_start();
        if !trimmed.starts_with('/') {
            return None;
        }
        let leading = before_cursor.len() - trimmed.len();
        Some(leading)
    }

    fn suggestions_first_token(partial: &str) -> Vec<&'static str> {
        let p = partial.to_ascii_lowercase();
        let mut hits: Vec<&str> = SLASH_COMMANDS
            .iter()
            .copied()
            .filter(|c| p.is_empty() || c.to_ascii_lowercase().starts_with(&p))
            .collect();
        hits.sort_unstable();
        hits.dedup();
        hits
    }

    fn suggestion_slash_command(span: Span, cmd: &str) -> Suggestion {
        let value = if cmd == "?" {
            "/?".to_string()
        } else {
            format!("/{cmd}")
        };
        Suggestion {
            value,
            span,
            append_whitespace: false,
            ..Default::default()
        }
    }

    fn suggestions_session_export_formats(span: Span, prefix: &str) -> Vec<Suggestion> {
        EXPORT_FORMAT_ARGS
            .iter()
            .copied()
            .map(|a| Suggestion {
                value: format!("{prefix} {a}"),
                span,
                append_whitespace: false,
                ..Default::default()
            })
            .collect()
    }
}

impl Completer for ReplSlashCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        if self.shell_mode.load(Ordering::Relaxed) {
            return vec![];
        }
        let pos = pos.min(line.len());
        let before = &line[..pos];
        let Some(slash_idx) = Self::slash_command_start_byte(before) else {
            return vec![];
        };
        let tail = &line[slash_idx + 1..pos];
        let span = Span::new(slash_idx, pos);

        match tail.split_once(char::is_whitespace) {
            None => {
                if tail.eq_ignore_ascii_case("export") {
                    return Self::suggestions_session_export_formats(span, "/export");
                }
                if tail.eq_ignore_ascii_case("save-session") {
                    return Self::suggestions_session_export_formats(span, "/save-session");
                }
                if tail.eq_ignore_ascii_case("config") {
                    return ["reload"]
                        .iter()
                        .map(|a| Suggestion {
                            value: format!("/config {a}"),
                            span,
                            append_whitespace: false,
                            ..Default::default()
                        })
                        .collect();
                }
                if tail.eq_ignore_ascii_case("mcp") {
                    return ["list", "probe"]
                        .iter()
                        .map(|a| Suggestion {
                            value: format!("/mcp {a}"),
                            span,
                            append_whitespace: false,
                            ..Default::default()
                        })
                        .collect();
                }
                if tail.eq_ignore_ascii_case("models") {
                    return ["list", "choose"]
                        .iter()
                        .map(|a| {
                            let value = if *a == "choose" {
                                format!("/models {a} ")
                            } else {
                                format!("/models {a}")
                            };
                            Suggestion {
                                value,
                                span,
                                append_whitespace: false,
                                ..Default::default()
                            }
                        })
                        .collect();
                }
                if tail.eq_ignore_ascii_case("agent") {
                    return ["list", "set"]
                        .iter()
                        .map(|a| {
                            let value = if *a == "set" {
                                format!("/agent {a} ")
                            } else {
                                format!("/agent {a}")
                            };
                            Suggestion {
                                value,
                                span,
                                append_whitespace: false,
                                ..Default::default()
                            }
                        })
                        .collect();
                }
                Self::suggestions_first_token(tail)
                    .into_iter()
                    .map(|cmd| Self::suggestion_slash_command(span, cmd))
                    .collect()
            }
            Some((cmd, after_ws)) => {
                let cmd = cmd.trim();
                if cmd.eq_ignore_ascii_case("config") {
                    let ap = after_ws.trim_start();
                    let ap_l = ap.to_ascii_lowercase();
                    let hits: Vec<&str> = if ap_l.is_empty() || "reload".starts_with(ap_l.as_str())
                    {
                        vec!["reload"]
                    } else {
                        vec![]
                    };
                    return hits
                        .into_iter()
                        .map(|a| Suggestion {
                            value: format!("/config {a}"),
                            span,
                            append_whitespace: false,
                            ..Default::default()
                        })
                        .collect();
                }
                if cmd.eq_ignore_ascii_case("mcp") {
                    let ap = after_ws.trim_start();
                    let ap_l = ap.to_ascii_lowercase();
                    let hits: Vec<&str> = if ap_l.is_empty() {
                        vec!["list", "probe"]
                    } else if ap_l.starts_with("list") {
                        let rest = ap_l.strip_prefix("list").unwrap_or("").trim_start();
                        if rest.is_empty() {
                            vec!["list", "list probe"]
                        } else if "probe".starts_with(rest) {
                            vec!["list probe"]
                        } else {
                            vec![]
                        }
                    } else {
                        ["list", "probe"]
                            .iter()
                            .copied()
                            .filter(|s| s.starts_with(ap_l.as_str()))
                            .collect()
                    };
                    return hits
                        .into_iter()
                        .map(|a| Suggestion {
                            value: format!("/mcp {a}"),
                            span,
                            append_whitespace: false,
                            ..Default::default()
                        })
                        .collect();
                }
                if cmd.eq_ignore_ascii_case("models") {
                    let ap = after_ws.trim_start();
                    let ap_l = ap.to_ascii_lowercase();
                    let hits: Vec<&str> = if ap_l.is_empty() {
                        vec!["list", "choose"]
                    } else {
                        ["list", "choose"]
                            .iter()
                            .copied()
                            .filter(|s| s.starts_with(ap_l.as_str()))
                            .collect()
                    };
                    return hits
                        .into_iter()
                        .map(|a| {
                            let value = if a == "choose" {
                                format!("/models {a} ")
                            } else {
                                format!("/models {a}")
                            };
                            Suggestion {
                                value,
                                span,
                                append_whitespace: false,
                                ..Default::default()
                            }
                        })
                        .collect();
                }
                if cmd.eq_ignore_ascii_case("agent") {
                    let ap = after_ws.trim_start();
                    let ap_l = ap.to_ascii_lowercase();
                    let hits: Vec<&str> = if ap_l.is_empty() {
                        vec!["list", "set"]
                    } else {
                        ["list", "set"]
                            .iter()
                            .copied()
                            .filter(|s| s.starts_with(ap_l.as_str()))
                            .collect()
                    };
                    return hits
                        .into_iter()
                        .map(|a| {
                            let value = if a == "set" {
                                format!("/agent {a} ")
                            } else {
                                format!("/agent {a}")
                            };
                            Suggestion {
                                value,
                                span,
                                append_whitespace: false,
                                ..Default::default()
                            }
                        })
                        .collect();
                }
                let prefix = if cmd.eq_ignore_ascii_case("export") {
                    "/export"
                } else if cmd.eq_ignore_ascii_case("save-session") {
                    "/save-session"
                } else {
                    return vec![];
                };
                let arg_prefix = after_ws.trim_start();
                let p = arg_prefix.to_ascii_lowercase();
                EXPORT_FORMAT_ARGS
                    .iter()
                    .copied()
                    .filter(|a| p.is_empty() || a.to_ascii_lowercase().starts_with(&p))
                    .map(|a| Suggestion {
                        value: format!("{prefix} {a}"),
                        span,
                        append_whitespace: false,
                        ..Default::default()
                    })
                    .collect()
            }
        }
    }
}

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

fn reedline_current_buffer_effectively_empty() -> bool {
    REEDLINE_BUFFER_PROBE.with(|c| {
        c.get()
            .is_some_and(|p| unsafe { (*p).current_buffer_contents().trim().is_empty() })
    })
}

/// `$` / `＄` 用于切换 shell 模式时接受的修饰键组合。
fn key_is_plain_dollar(code: &KeyCode, modifiers: KeyModifiers) -> bool {
    let ch = match code {
        KeyCode::Char(c) => *c,
        _ => return false,
    };
    if ch != '$' && ch != '\u{ff04}' {
        return false;
    }
    if modifiers.intersects(KeyModifiers::SUPER | KeyModifiers::HYPER) {
        return false;
    }
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);
    // 拒绝仅 Ctrl 或仅 Alt（避免误绑）；保留 Ctrl+Alt（Windows/Linux 上 AltGr 常见）。
    if ctrl ^ alt {
        return false;
    }
    true
}

/// 缓冲区为空时按 `$` / `＄` 立即 [`ReedlineEvent::ExecuteHostCommand`]（虚拟行 `"$"`），其余键委托 **Emacs**。
struct DollarToggleEmacs {
    emacs: Emacs,
}

impl DollarToggleEmacs {
    fn new() -> Self {
        Self {
            emacs: Emacs::new(repl_emacs_keybindings()),
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
            && reedline_current_buffer_effectively_empty()
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
    // IDE / 管道模式下 stdin 非 TTY：reedline 不会绘制提示符，否则看起来像「无输入界面」。
    if shell_mode.load(Ordering::Relaxed) {
        print!("bash#:{}", CLI_PROMPT_AFTER_COLON);
    } else {
        print!("我:{}", CLI_PROMPT_AFTER_COLON);
    }
    io::stdout().flush()?;

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

        let completion_menu = Box::new(ColumnarMenu::default().with_name(REPL_COMPLETION_MENU));
        let completer = Box::new(ReplSlashCompleter::new(shell_mode.clone()));

        let reedline = Reedline::create()
            .with_history(Box::new(history))
            .with_ansi_colors(ansi_prompt)
            .with_completer(completer)
            .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
            .with_quick_completions(true)
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

#[cfg(test)]
mod dollar_key_tests {
    use super::key_is_plain_dollar;
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn dollar_accepts_shift_and_altgr_style_modifiers() {
        assert!(key_is_plain_dollar(&KeyCode::Char('$'), KeyModifiers::NONE));
        assert!(key_is_plain_dollar(
            &KeyCode::Char('$'),
            KeyModifiers::SHIFT
        ));
        assert!(key_is_plain_dollar(
            &KeyCode::Char('$'),
            KeyModifiers::CONTROL | KeyModifiers::ALT
        ));
    }

    #[test]
    fn dollar_rejects_ctrl_or_alt_alone() {
        assert!(!key_is_plain_dollar(
            &KeyCode::Char('$'),
            KeyModifiers::CONTROL
        ));
        assert!(!key_is_plain_dollar(&KeyCode::Char('$'), KeyModifiers::ALT));
    }

    #[test]
    fn dollar_rejects_non_dollar_chars() {
        assert!(!key_is_plain_dollar(
            &KeyCode::Char('4'),
            KeyModifiers::SHIFT
        ));
    }
}

#[cfg(test)]
mod slash_completion_tests {
    use super::ReplSlashCompleter;
    use reedline::Completer;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn shell_mode_disables_completion() {
        let mut c = ReplSlashCompleter::new(Arc::new(AtomicBool::new(true)));
        assert!(c.complete("/cle", 4).is_empty());
    }

    #[test]
    fn no_completion_when_slash_not_at_line_start() {
        let mut c = ReplSlashCompleter::new(Arc::new(AtomicBool::new(false)));
        assert!(c.complete("say /clear", 10).is_empty());
    }

    #[test]
    fn completes_clear_from_partial() {
        let mut c = ReplSlashCompleter::new(Arc::new(AtomicBool::new(false)));
        let s = c.complete("/cle", 4);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].value, "/clear");
    }

    #[test]
    fn export_then_formats() {
        let mut c = ReplSlashCompleter::new(Arc::new(AtomicBool::new(false)));
        let s = c.complete("/export", 7);
        assert!(s.iter().any(|x| x.value == "/export json"));
        let s2 = c.complete("/export j", 9);
        assert_eq!(s2.len(), 1);
        assert_eq!(s2[0].value, "/export json");
    }

    #[test]
    fn save_session_then_formats() {
        let mut c = ReplSlashCompleter::new(Arc::new(AtomicBool::new(false)));
        let line = "/save-session";
        let s = c.complete(line, line.len());
        assert!(s.iter().any(|x| x.value == "/save-session json"));
        let line2 = "/save-session j";
        let s2 = c.complete(line2, line2.len());
        assert_eq!(s2.len(), 1);
        assert_eq!(s2[0].value, "/save-session json");
    }

    #[test]
    fn config_reload_completion() {
        let mut c = ReplSlashCompleter::new(Arc::new(AtomicBool::new(false)));
        let s = c.complete("/config", 7);
        assert!(s.iter().any(|x| x.value == "/config reload"));
        let line = "/config ";
        let s2 = c.complete(line, line.len());
        assert!(s2.iter().any(|x| x.value == "/config reload"));
    }

    #[test]
    fn mcp_subcommands() {
        let mut c = ReplSlashCompleter::new(Arc::new(AtomicBool::new(false)));
        let s = c.complete("/mcp", 4);
        assert!(s.iter().any(|x| x.value == "/mcp list"));
        assert!(s.iter().any(|x| x.value == "/mcp probe"));
        let line = "/mcp list ";
        let s2 = c.complete(line, line.len());
        assert!(s2.iter().any(|x| x.value == "/mcp list probe"));
        let line3 = "/mcp list p";
        let s3 = c.complete(line3, line3.len());
        assert_eq!(s3.len(), 1);
        assert_eq!(s3[0].value, "/mcp list probe");
    }

    #[test]
    fn models_subcommands() {
        let mut c = ReplSlashCompleter::new(Arc::new(AtomicBool::new(false)));
        let s = c.complete("/models", 7);
        assert!(s.iter().any(|x| x.value == "/models list"));
        assert!(s.iter().any(|x| x.value == "/models choose "));
        let line = "/models ";
        let s2 = c.complete(line, line.len());
        assert!(s2.iter().any(|x| x.value == "/models list"));
        assert!(s2.iter().any(|x| x.value == "/models choose "));
    }

    #[test]
    fn agent_subcommands() {
        let mut c = ReplSlashCompleter::new(Arc::new(AtomicBool::new(false)));
        let s = c.complete("/agent", 6);
        assert!(s.iter().any(|x| x.value == "/agent list"));
        assert!(s.iter().any(|x| x.value == "/agent set "));
        let line = "/agent ";
        let s2 = c.complete(line, line.len());
        assert!(s2.iter().any(|x| x.value == "/agent list"));
        assert!(s2.iter().any(|x| x.value == "/agent set "));
    }
}
