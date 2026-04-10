//! 与 `GET /web-ui` 对齐的进程环境变量；供 Web handler 与分阶段规划 SSE 等共用。

/// 为真时 CSR 不对助手消息做展示层过滤；**同时**允许无工具规划轮向浏览器流式下发原文（默认不下发）。
pub(crate) const AGENT_WEB_RAW_ASSISTANT_OUTPUT: &str = "AGENT_WEB_RAW_ASSISTANT_OUTPUT";

pub(crate) fn web_raw_assistant_output_env() -> bool {
    match std::env::var(AGENT_WEB_RAW_ASSISTANT_OUTPUT) {
        Ok(s) => {
            let t = s.trim().to_ascii_lowercase();
            matches!(t.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}
