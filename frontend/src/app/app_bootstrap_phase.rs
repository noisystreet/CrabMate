//! 壳层首启与 `GET /web-ui` 门闸的**显式阶段**（与 `initialized` + `web_ui_config_loaded` 两枚信号对应）。
//!
//! 水合等副作用依赖「本地会话已就绪且 Web UI 配置路径已触发」；用枚举表达合法组合，避免散落
//! `if !initialized || !web_ui_config_loaded` 与真实阶段语义漂移。
//!
//! 阶段顺序见 [`crate::app::chat::wire_chat_session_lifecycle::wire_chat_session_lifecycle_effects`] 内 `wire_*` 注册表。

/// 从 `localStorage` 首载到可安全跑会话水合等壳级 `Effect` 的粗粒度阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppBootstrapPhase {
    /// 尚未执行 [`crate::app::chat::session_storage::wire_initial_sessions_from_storage`] 完成首帧。
    Cold,
    /// `initialized == true`：`sessions` / `active_id` / 草稿已从本地桶物化。
    SessionsMaterialized,
    /// `initialized && web_ui_config_loaded`：已走过 [`crate::app::chat::session_storage::wire_web_ui_config_once_after_init`] 的门闸（`GET /web-ui` 可能仍在飞行）。
    ShellBootstrapComplete,
}

impl AppBootstrapPhase {
    #[must_use]
    pub(crate) fn derive(initialized: bool, web_ui_config_loaded: bool) -> Self {
        if !initialized {
            Self::Cold
        } else if !web_ui_config_loaded {
            Self::SessionsMaterialized
        } else {
            Self::ShellBootstrapComplete
        }
    }

    /// 与 [`crate::app::chat::session_hydrate::wire_session_hydration`] 历史条件一致：仅在此阶段订阅并拉取服务端快照。
    #[must_use]
    pub(crate) const fn hydration_effects_enabled(self) -> bool {
        matches!(self, Self::ShellBootstrapComplete)
    }
}
