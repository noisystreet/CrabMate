//! IDE 内置文本编辑器本机偏好（`localStorage`）。

use crate::app_prefs::{
    IDE_EDITOR_FONT_KEY, IDE_EDITOR_FONT_SIZE_KEY, IDE_EDITOR_LINE_NUMBERS_KEY,
    IDE_EDITOR_TAB_SIZE_KEY, IDE_EDITOR_WORD_WRAP_KEY, load_bool_key, local_storage,
    store_bool_key, store_f64_key,
};

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

impl IdeEditorPrefs {
    #[must_use]
    pub fn load() -> Self {
        let font_slug = local_storage()
            .and_then(|st| st.get_item(IDE_EDITOR_FONT_KEY).ok().flatten())
            .map(|s| normalize_font_slug(&s))
            .unwrap_or_else(|| DEFAULT_IDE_EDITOR_FONT.to_string());
        let font_size_px =
            load_f64_key_unclamped(IDE_EDITOR_FONT_SIZE_KEY, DEFAULT_IDE_EDITOR_FONT_SIZE)
                .clamp(10.0, 28.0);
        let line_numbers = load_bool_key(IDE_EDITOR_LINE_NUMBERS_KEY, true);
        let word_wrap = load_bool_key(IDE_EDITOR_WORD_WRAP_KEY, false);
        let tab_size = load_f64_key_unclamped(IDE_EDITOR_TAB_SIZE_KEY, DEFAULT_IDE_EDITOR_TAB_SIZE)
            .round()
            .clamp(2.0, 8.0) as u8;
        Self {
            font_slug,
            font_size_px,
            line_numbers,
            word_wrap,
            tab_size,
        }
    }

    pub fn persist(&self) {
        if let Some(st) = local_storage() {
            let _ = st.set_item(
                IDE_EDITOR_FONT_KEY,
                normalize_font_slug(&self.font_slug).as_str(),
            );
        }
        store_f64_key(IDE_EDITOR_FONT_SIZE_KEY, self.font_size_px);
        store_bool_key(IDE_EDITOR_LINE_NUMBERS_KEY, self.line_numbers);
        store_bool_key(IDE_EDITOR_WORD_WRAP_KEY, self.word_wrap);
        store_f64_key(IDE_EDITOR_TAB_SIZE_KEY, f64::from(self.tab_size));
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

fn load_f64_key_unclamped(key: &str, default: f64) -> f64 {
    let Some(st) = local_storage() else {
        return default;
    };
    let Ok(Some(v)) = st.get_item(key) else {
        return default;
    };
    v.parse::<f64>().unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_font_slug_falls_back() {
        assert_eq!(
            normalize_font_slug("nope").as_str(),
            DEFAULT_IDE_EDITOR_FONT
        );
    }
}
