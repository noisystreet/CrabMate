//! 本机 `localStorage`：**句柄**统一自 [`crate::app_prefs::local_storage`]；本文件只做键空间索引，避免再写 `web_sys::window()?.local_storage()`。
//!
//! ## 键归属（实现模块）
//!
//! | 主题 | 模块 |
//! |------|------|
//! | 主题、背景、侧栏宽度、`agent-demo-*` 等壳布局 | [`crate::app_prefs`]、[`super::shell_prefs_storage`] |
//! | `crabmate-locale` | [`crate::i18n::locale_storage`] |
//! | `crabmate-client-llm-*`、`crabmate-executor-*`、执行模式 | [`crate::api::client_llm_storage`] |
//! | 会话 JSON、`active_id` | [`crate::storage`] |
//! | Web API Bearer（与 `localStorage` 同句柄） | [`crate::api::browser`] |
//!
//! 新增持久化键时更新上表；取 `Storage` 请用 [`handle`] 或 `app_prefs::local_storage`。

#[inline]
pub(crate) fn handle() -> Option<web_sys::Storage> {
    crate::app_prefs::local_storage()
}
