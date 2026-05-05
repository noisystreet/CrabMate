//! 聊天区程序化滚底时与 `on:scroll` 的协调标记。

use leptos::prelude::*;

/// 跟底 Effect 程序化 `set_scroll_top` 期间为 true；Drop 时清除，避免 `on:scroll` 误判 gap。
pub struct MessagesScrollFromEffectGuard {
    flag: RwSignal<bool>,
}

impl MessagesScrollFromEffectGuard {
    pub fn new(flag: RwSignal<bool>) -> Self {
        flag.set(true);
        Self { flag }
    }
}

impl Drop for MessagesScrollFromEffectGuard {
    fn drop(&mut self) {
        self.flag.set(false);
    }
}
