use super::Locale;

pub fn github_embed_back(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "返回",
        Locale::En => "Back",
    }
}

pub fn github_embed_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "GitHub",
        Locale::En => "GitHub",
    }
}

pub fn github_embed_browser_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "浏览器模式无法在页面内嵌入 GitHub，请使用下方按钮在系统浏览器中打开。",
        Locale::En => {
            "GitHub cannot be embedded in the browser shell; use the button below to open it in your system browser."
        }
    }
}

pub fn github_embed_open_browser(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在浏览器中打开",
        Locale::En => "Open in browser",
    }
}
