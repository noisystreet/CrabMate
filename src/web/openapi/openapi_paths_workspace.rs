//! `/workspace/file/raw` OpenAPI 片段（从 `openapi_paths.rs` 拆出以控制文件行数）。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_workspace_file_raw() -> Value {
    json!({
        "/workspace/file/raw": {
            "get": {
                "tags": ["workspace"],
                "summary": "读取工作区内常见图片原始字节（png/jpg/jpeg/webp/gif；上限 8 MiB）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "path",
                        "in": "query",
                        "required": true,
                        "schema": { "type": "string" },
                        "description": "工作区相对路径；禁止 `..`；不含 svg"
                    }
                ],
                "responses": {
                    "200": {
                        "description": "图片字节",
                        "content": {
                            "image/png": { "schema": { "type": "string", "format": "binary" } },
                            "image/jpeg": { "schema": { "type": "string", "format": "binary" } },
                            "image/webp": { "schema": { "type": "string", "format": "binary" } },
                            "image/gif": { "schema": { "type": "string", "format": "binary" } }
                        }
                    },
                    "4XX": { "description": "路径、类型或大小错误（JSON ApiError）" }
                }
            }
        }
    })
}
