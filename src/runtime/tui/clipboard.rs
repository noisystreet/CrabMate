//! 系统剪贴板读取（TUI 粘贴）。Wayland/X11 等需系统提供剪贴板实现（如 `wl-clipboard`、`xclip`）。

use arboard::Clipboard;

/// 单次粘贴最大字符数（按 UTF-8 字节截断到字符边界）。
const MAX_PASTE_BYTES: usize = 256 * 1024;

pub(super) fn try_clipboard_text() -> Option<String> {
    let mut text = Clipboard::new().ok()?.get_text().ok()?;
    text.retain(|c| c != '\0');
    if text.len() > MAX_PASTE_BYTES {
        text.truncate(MAX_PASTE_BYTES);
        while !text.is_char_boundary(text.len()) {
            text.pop();
        }
    }
    if text.is_empty() { None } else { Some(text) }
}
