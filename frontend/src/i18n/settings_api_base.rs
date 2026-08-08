//! 设置页：远程 serve API 基址文案（从 `settings` 拆出以控制单文件行数）。

use super::Locale;

pub fn settings_block_api_base(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "API 基址（远程 serve）",
        Locale::En => "API base URL (remote serve)",
    }
}

pub fn settings_api_base_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "静态托管本 UI、API 在另一 Origin 时填写 serve 根地址（如 http://127.0.0.1:8080）。留空则同 Origin（相对路径）。须同时配置上方 Web Bearer，且 serve 开启 CORS 白名单。不是模型 api_base。"
        }
        Locale::En => {
            "When this UI is hosted separately from serve, set the serve root (e.g. http://127.0.0.1:8080). Leave empty for same-origin relative paths. Also set Web Bearer above and allow this Origin in serve CORS. Not the LLM api_base."
        }
    }
}

pub fn settings_api_base_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "API 基址",
        Locale::En => "API base URL",
    }
}

pub fn settings_api_base_save(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存 API 基址",
        Locale::En => "Save API base",
    }
}

pub fn settings_api_base_saved(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已保存 API 基址；正在重新拉取状态…",
        Locale::En => "API base saved; refreshing status…",
    }
}

pub fn settings_api_base_cleared(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已清空；改回同 Origin 相对路径。",
        Locale::En => "Cleared; back to same-origin relative paths.",
    }
}

pub fn settings_api_base_invalid(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无效地址：须为 http:// 或 https:// 开头（或留空）。",
        Locale::En => "Invalid URL: must start with http:// or https:// (or leave empty).",
    }
}
