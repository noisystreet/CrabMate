//! OpenAPI `paths` 中后台工具任务端点片段（`/tools/jobs/*`）。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_tool_jobs() -> Value {
    json!({
        "/tools/jobs/{tool_job_id}": {
            "get": {
                "tags": ["tool_jobs"],
                "summary": "轮询后台工具任务状态与（终态）输出",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "tool_job_id",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" }
                    },
                    {
                        "name": "X-Workspace-Root",
                        "in": "header",
                        "required": false,
                        "description": "可选归属校验：与任务创建时记录的工作区比对，不符 403 JOB_OWNERSHIP_MISMATCH",
                        "schema": { "type": "string" }
                    }
                ],
                "responses": {
                    "200": {
                        "description": "任务状态快照",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ToolJobStatusResponseBody" }
                            }
                        }
                    },
                    "403": { "description": "X-Workspace-Root 与任务归属不符（JOB_OWNERSHIP_MISMATCH）" },
                    "404": { "description": "不存在 / 从未创建（JOB_NOT_FOUND）" },
                    "410": { "description": "已过 TTL+宽限被清理（JOB_EXPIRED）" }
                }
            }
        },
        "/tools/jobs/{tool_job_id}/cancel": {
            "post": {
                "tags": ["tool_jobs"],
                "summary": "取消后台工具任务（queued 直接转移；running 置取消标记）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "tool_job_id",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" }
                    },
                    {
                        "name": "X-Workspace-Root",
                        "in": "header",
                        "required": false,
                        "description": "可选归属校验：与任务创建时记录的工作区比对，不符 403 JOB_OWNERSHIP_MISMATCH",
                        "schema": { "type": "string" }
                    }
                ],
                "responses": {
                    "200": {
                        "description": "已取消（幂等；已是 cancelled 亦 200）",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ToolJobCancelResponseBody" }
                            }
                        }
                    },
                    "403": { "description": "X-Workspace-Root 与任务归属不符（JOB_OWNERSHIP_MISMATCH）" },
                    "404": { "description": "不存在 / 从未创建（JOB_NOT_FOUND）" },
                    "409": { "description": "已是其它终态（不覆盖；body `{ \"status\": <当前状态> }`）" },
                    "410": { "description": "已过 TTL+宽限被清理（JOB_EXPIRED）" }
                }
            }
        }
    })
}
