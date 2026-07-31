//! 设置页主题 / 背景预览写 DOM（从 `settings_page` 拆出以降低 nloc 棘轮）。

pub(crate) fn apply_theme_preview_to_dom(theme_pref: &str) {
    crate::app::shell_prefs_storage::persist_theme_to_storage_and_dom(theme_pref);
}

pub(crate) fn apply_bg_decor_preview_to_dom(bg_decor: bool) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document())
        && let Some(root) = doc.document_element()
    {
        if bg_decor {
            let _ = root.remove_attribute("data-bg-decor");
        } else {
            let _ = root.set_attribute("data-bg-decor", "plain");
        }
    }
}
