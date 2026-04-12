use crate::app_prefs::{LOCALE_KEY, local_storage};

/// 界面语言（与 `<html lang>` 对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locale {
    ZhHans,
    En,
}

impl Locale {
    pub fn from_storage_slug(s: &str) -> Self {
        match s.trim() {
            "en" => Locale::En,
            _ => Locale::ZhHans,
        }
    }

    pub fn html_lang(self) -> &'static str {
        match self {
            Locale::ZhHans => "zh-Hans",
            Locale::En => "en",
        }
    }

    pub fn storage_slug(self) -> &'static str {
        match self {
            Locale::ZhHans => "zh-Hans",
            Locale::En => "en",
        }
    }
}

pub fn load_locale_from_storage() -> Locale {
    local_storage()
        .and_then(|s| s.get_item(LOCALE_KEY).ok().flatten())
        .map(|v| Locale::from_storage_slug(&v))
        .unwrap_or(Locale::ZhHans)
}

pub fn store_locale_slug(slug: &str) {
    if let Some(st) = local_storage() {
        let _ = st.set_item(LOCALE_KEY, slug);
    }
}
