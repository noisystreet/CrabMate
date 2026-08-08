//! 连接成功后允许留在 WebView 内的 `serve` Origin（防止连接页任意 http 导航）。

use std::sync::Mutex;

use url::{Origin, Url};

/// 最近一次 `connect_remote` 探测成功后的目标 Origin。
#[derive(Debug, Default)]
pub struct AllowedServeOrigin(Mutex<Option<Origin>>);

impl AllowedServeOrigin {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn set_from_url(&self, url: &Url) {
        if let Ok(mut g) = self.0.lock() {
            *g = Some(url.origin());
        }
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.0.lock() {
            *g = None;
        }
    }

    #[must_use]
    pub fn matches_url(&self, url: &Url) -> bool {
        self.0
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .is_some_and(|o| o == url.origin())
    }
}
