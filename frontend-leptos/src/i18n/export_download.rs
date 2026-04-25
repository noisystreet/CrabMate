//! 浏览器 / Tauri 导出与下载相关提示。

use super::Locale;

pub fn export_tauri_save_cancelled_alert(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已取消保存。",
        Locale::En => "Save cancelled.",
    }
}

pub fn export_tauri_save_failed_alert(l: Locale, err: &str) -> String {
    match l {
        Locale::ZhHans => format!("导出失败（Tauri 保存对话框）：{err}"),
        Locale::En => format!("Export failed (Tauri save dialog): {err}"),
    }
}
