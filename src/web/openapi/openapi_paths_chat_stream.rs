//! OpenAPI `paths`：`POST /chat/stream/{job_id}/cancel`（从 `openapi_paths.rs` 拆出以控文件行数）。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_chat_stream_cancel() -> Value {
    json!({
        "/chat/stream/{job_id}/cancel": {
            "post": {
                "tags": ["chat"],
                "summary": "取消进行中的 SSE 流式回合",
                "description": "置位与 `x-stream-job-id` 相同的协作取消标志，并取消该回合发起的后台 `run_command` 任务。仅断开 SSE **不会**取消（以便 `stream_resume`）。任务已结束或不存在则 410 `STREAM_JOB_GONE`（与 `stream_resume` 同码）。",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "job_id",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "integer", "format": "int64" }
                    }
                ],
                "responses": {
                    "200": {
                        "description": "已请求取消",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["job_id", "cancelled", "background_tools_cancelled"],
                                    "properties": {
                                        "job_id": { "type": "integer", "format": "int64" },
                                        "cancelled": { "type": "boolean" },
                                        "background_tools_cancelled": { "type": "integer" }
                                    }
                                }
                            }
                        }
                    },
                    "410": { "description": "任务已结束或不存在（STREAM_JOB_GONE）" }
                }
            }
        }
    })
}
