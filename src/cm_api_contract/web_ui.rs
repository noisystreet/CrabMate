//! `GET /web-ui` 响应：CSR 展示开关（只读进程环境变量）。

use schemars::JsonSchema;
use serde::Serialize;

/// `GET /web-ui` JSON（`markdown_render` / `apply_assistant_display_filters`）。
#[derive(Serialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct WebUiConfigResponse {
    /// 为 `false` 时 Web CSR 应以纯文本（HTML 转义）展示聊天气泡，跳过 Markdown 解析（进程环境 **`CM_WEB_DISABLE_MARKDOWN`**）。
    pub markdown_render: bool,
    /// 为 `false` 时 CSR 不对助手消息做展示层过滤，按存储原文输出（进程环境 **`CM_WEB_RAW_ASSISTANT_OUTPUT`**）。
    pub apply_assistant_display_filters: bool,
}
