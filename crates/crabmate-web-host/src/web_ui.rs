//! `GET /web-ui`：CSR 展示开关（仅读进程环境变量，无 TOML）。

use axum::Json;
pub use crabmate_api_contract::WebUiConfigResponse;

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
        let r = WebUiConfigResponse {
            markdown_render: true,
            apply_assistant_display_filters: true,
        };
        assert!(r.markdown_render);
        assert!(r.apply_assistant_display_filters);
    }
}
