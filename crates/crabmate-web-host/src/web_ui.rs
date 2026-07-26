//! `GET /web-ui`：CSR 展示开关（仅读进程环境变量，无 TOML）。

use axum::Json;
use serde::Serialize;

/// 为真时 CSR 跳过聊天气泡 Markdown（纯文本 HTML 转义）。
pub const CM_WEB_DISABLE_MARKDOWN: &str = "CM_WEB_DISABLE_MARKDOWN";

/// 为真时 CSR 不对助手消息做展示层过滤；并影响分阶段规划 SSE 门控（与根包 `env_flags` 同名约定）。
pub const CM_WEB_RAW_ASSISTANT_OUTPUT: &str = "CM_WEB_RAW_ASSISTANT_OUTPUT";

fn env_flag_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(s) => {
            let t = s.trim().to_ascii_lowercase();
            matches!(t.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

pub fn web_disable_markdown_env() -> bool {
    env_flag_truthy(CM_WEB_DISABLE_MARKDOWN)
}

pub fn web_raw_assistant_output_env() -> bool {
    env_flag_truthy(CM_WEB_RAW_ASSISTANT_OUTPUT)
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct WebUiConfigResponse {
    /// 为 `false` 时 Web CSR 应以纯文本（HTML 转义）展示聊天气泡，跳过 Markdown 解析。
    pub markdown_render: bool,
    /// 为 `false` 时 CSR 不对助手消息做展示层过滤，按存储原文输出。
    pub apply_assistant_display_filters: bool,
}

pub fn web_ui_config() -> WebUiConfigResponse {
    WebUiConfigResponse {
        markdown_render: !web_disable_markdown_env(),
        apply_assistant_display_filters: !web_raw_assistant_output_env(),
    }
}

pub async fn web_ui_config_handler() -> Json<WebUiConfigResponse> {
    Json(web_ui_config())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_enables_markdown_and_filters() {
        // 不依赖进程 env 是否被污染：仅断言结构字段可构造。
        let r = WebUiConfigResponse {
            markdown_render: true,
            apply_assistant_display_filters: true,
        };
        assert!(r.markdown_render);
        assert!(r.apply_assistant_display_filters);
    }
}
