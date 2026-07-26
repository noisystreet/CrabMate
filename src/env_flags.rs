//! 与 `GET /web-ui` 对齐的进程环境变量；供 Web handler 与分阶段规划 SSE 等共用。
//!
//! `web` feature 下与 **`crabmate-web-host::web_ui`** 同源；非 web 构建保留本地实现。

#[cfg(feature = "web")]
pub(crate) use crabmate_web_host::web_ui::{
    CM_WEB_RAW_ASSISTANT_OUTPUT, web_raw_assistant_output_env,
};

#[cfg(not(feature = "web"))]
/// 为真时 CSR 不对助手消息做展示层过滤；**同时**允许无工具规划轮向浏览器流式下发原文（默认不下发）。
pub(crate) const CM_WEB_RAW_ASSISTANT_OUTPUT: &str = "CM_WEB_RAW_ASSISTANT_OUTPUT";

#[cfg(not(feature = "web"))]
pub(crate) fn web_raw_assistant_output_env() -> bool {
    match std::env::var(CM_WEB_RAW_ASSISTANT_OUTPUT) {
        Ok(s) => {
            let t = s.trim().to_ascii_lowercase();
            matches!(t.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}
