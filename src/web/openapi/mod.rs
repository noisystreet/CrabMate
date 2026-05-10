//! `GET /openapi.json`：OpenAPI 3.0 机器可读契约（与 `server.rs` 路由对齐；**不**替代 `docs/SSE协议.md` 对 SSE 行级语义的说明）。

mod openapi_components;
mod openapi_paths;

use serde_json::{Value, json};

use openapi_components::openapi_components_value;
use openapi_paths::openapi_paths_value;

/// 构建与当前 `serve` 路由表一致的 OpenAPI 文档（不含静态 `/`、`/uploads` 文件服务细节）。
pub fn build_openapi_spec() -> Value {
    let version = env!("CARGO_PKG_VERSION");
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "CrabMate Web API",
            "version": version,
            "description": concat!(
                "CrabMate `serve` 模式的 HTTP 契约摘要。\n\n",
                "- **鉴权**：嵌入默认 **`web_api_require_bearer=false`**：允许无共享密钥启动 **`serve`**；若将 **`web_api_require_bearer=true`**，则启动前须配置非空 **`CM_WEB_API_BEARER_TOKEN`**（或 TOML **`web_api_bearer_token`**）。进程启动且密钥**非空**时，下列需鉴权路径须在请求头携带 **`Authorization: Bearer <token>`** 或 **`X-API-Key: <token>`**（与配置值为**同一密钥**，二选一）。密钥为空时中间件不校验，路径可对能访问监听地址的客户端匿名访问（仅限可信环境）。\n",
                "- **SSE**：`POST /chat/stream` 返回 `text/event-stream`；控制面 JSON 与错误码见仓库 `docs/SSE协议.md`，本 OpenAPI 仅作入口说明。\n",
                "- **上传**：`POST /upload` 使用 `multipart/form-data`。"
            )
        },
        "tags": [
            { "name": "chat", "description": "对话与流式 SSE" },
            { "name": "workspace", "description": "工作区浏览与文件" },
            { "name": "system", "description": "健康检查与状态" },
            { "name": "tasks", "description": "进程内任务清单" },
            { "name": "config", "description": "配置热重载" },
            { "name": "uploads", "description": "上传与删除" }
        ],
        "paths": openapi_paths_value(),
        "components": openapi_components_value(),
    })
}

/// Axum handler：`application/json` OpenAPI 文档。
pub(crate) async fn openapi_json_handler() -> axum::Json<Value> {
    axum::Json(build_openapi_spec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_spec_has_core_paths_and_version() {
        let v = build_openapi_spec();
        assert_eq!(v["openapi"], "3.0.3");
        let paths = v["paths"].as_object().expect("paths object");
        assert!(paths.contains_key("/health"));
        assert!(paths.contains_key("/web-ui"));
        assert!(paths.contains_key("/chat/stream"));
        assert!(paths.contains_key("/chat/async"));
        assert!(paths.contains_key("/chat/jobs/{job_id}"));
        assert!(paths.contains_key("/conversation/messages"));
        assert!(paths.contains_key("/openapi.json"));
        assert!(v["components"]["securitySchemes"]["bearerAuth"].is_object());
        assert!(v["components"]["securitySchemes"]["apiKeyAuth"].is_object());
    }
}
