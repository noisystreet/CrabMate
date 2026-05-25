//! IDE 内置文本编辑器偏好（持久化在 **`/user-data/prefs`**）。

pub const IDE_EDITOR_FONT_SLUGS: &[&str] = &["jetbrains", "cascadia", "fira", "system"];

pub const DEFAULT_IDE_EDITOR_FONT: &str = "jetbrains";
pub const DEFAULT_IDE_EDITOR_FONT_SIZE: f64 = 14.0;
pub const DEFAULT_IDE_EDITOR_TAB_SIZE: f64 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub struct IdeEditorPrefs {
    pub font_slug: String,
    pub font_size_px: f64,
    pub line_numbers: bool,
    pub word_wrap: bool,
    pub tab_size: u8,
}

impl Default for IdeEditorPrefs {
    fn default() -> Self {
        Self {
            font_slug: DEFAULT_IDE_EDITOR_FONT.to_string(),
            font_size_px: DEFAULT_IDE_EDITOR_FONT_SIZE,
            line_numbers: true,
            word_wrap: false,
            tab_size: DEFAULT_IDE_EDITOR_TAB_SIZE as u8,
        }
    }
}

impl IdeEditorPrefs {
    #[must_use]
    pub fn load() -> Self {
        Self::default()
    }

    pub fn persist(&self) {
        // 由 [`crate::user_prefs_sync`] 防抖写入 `/user-data/prefs`。
    }
}

#[must_use]
pub fn normalize_font_slug(raw: &str) -> String {
    let t = raw.trim();
    if IDE_EDITOR_FONT_SLUGS.contains(&t) {
        t.to_string()
    } else {
        DEFAULT_IDE_EDITOR_FONT.to_string()
    }
}

#[must_use]
pub fn ide_editor_font_family_css(slug: &str) -> &'static str {
    match normalize_font_slug(slug).as_str() {
        "cascadia" => "\"Cascadia Code\", ui-monospace, monospace",
        "fira" => "\"Fira Code\", ui-monospace, monospace",
        "system" => "ui-monospace, monospace",
        _ => "\"JetBrains Mono\", ui-monospace, monospace",
    }
}
