//! OpenAPI `components` 对象（由 `openapi::build_openapi_spec` 组装）。
//!
//! 拆成多段 `json!` + `merge_component_objects`，降低单函数 `nloc`（`fn-nloc` 棘轮）；运行时合并为单一 object。

use serde_json::{Map, Value, json};

use super::openapi_components_user_data;

fn openapi_components_schemas_from_contract() -> Value {
    Value::Object(crate::cm_api_contract::openapi::openapi_component_schemas())
}

fn merge_component_objects(fragments: &[Value]) -> Value {
    let mut map = Map::new();
    for fragment in fragments {
        let Value::Object(o) = fragment else {
            panic!("openapi components fragment must be a JSON object");
        };
        for (k, v) in o {
            if map.insert(k.clone(), v.clone()).is_some() {
                panic!("duplicate OpenAPI components/schemas key: {k}");
            }
        }
    }
    Value::Object(map)
}

fn openapi_components_security_schemes() -> Value {
    json!({
        "securitySchemes": {
            "bearerAuth": {
                "type": "http",
                "scheme": "bearer",
                "description": "与 `[agent].web_api_bearer_token` / `CM_WEB_API_BEARER_TOKEN` 一致；未启用服务端密钥时可为空。"
            },
            "apiKeyAuth": {
                "type": "apiKey",
                "in": "header",
                "name": "X-API-Key",
                "description": "与 `web_api_bearer_token` 相同密钥；与 Bearer 二选一即可（常见于 Dify / Open WebUI 类网关习惯）。"
            }
        }
    })
}

fn openapi_components_schemas_chat_llm_webui() -> Value {
    json!({
            "StatusResponseBody": {
                "type": "object",
                "description": "GET /status（无 view 参数）完整运行状态；字段集以后端实现为准，此处不逐项枚举",
                "additionalProperties": true
            },
    })
}

fn openapi_components_schemas_chat_core() -> Value {
    merge_component_objects(&[openapi_components_schemas_chat_llm_webui()])
}

fn openapi_components_schemas_workspace_tasks_config() -> Value {
    json!({
            "WorkspaceEntry": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "is_dir": { "type": "boolean" }
                }
            },
            "WorkspaceResponse": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "entries": {
                        "type": "array",
                        "items": { "$ref": "#/components/schemas/WorkspaceEntry" }
                    },
                    "error": { "type": "string", "nullable": true }
                }
            },
            "WorkspaceSetBody": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "nullable": true }
                }
            },
            "WorkspacePickResponse": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "nullable": true }
                }
            },
            "WorkspaceSearchBody": {
                "type": "object",
                "required": ["pattern"],
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "max_results": { "type": "integer" },
                    "case_insensitive": { "type": "boolean" },
                    "ignore_hidden": { "type": "boolean" }
                }
            },
            "WorkspaceSearchResponse": {
                "type": "object",
                "properties": {
                    "output": { "type": "string" },
                    "error": { "type": "string", "nullable": true }
                }
            },
            "WorkspaceProfileResponse": {
                "type": "object",
                "properties": {
                    "markdown": { "type": "string" },
                    "error": { "type": "string", "nullable": true }
                }
            },
            "SkillListItem": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string", "nullable": true },
                    "description": { "type": "string" },
                    "path": { "type": "string" }
                }
            },
            "SkillsListResponse": {
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "skills_dir": { "type": "string" },
                    "skills_user_dir": { "type": "string" },
                    "skills_system_dir": { "type": "string" },
                    "skills": {
                        "type": "array",
                        "items": { "$ref": "#/components/schemas/SkillListItem" }
                    },
                    "error": { "type": "string", "nullable": true }
                }
            },
            "WorkspaceChangelogResponse": {
                "type": "object",
                "properties": {
                    "revision": { "type": "integer", "format": "int64" },
                    "markdown": { "type": "string" },
                    "error": { "type": "string", "nullable": true }
                }
            },
            "WorkspaceFileMoveBody": {
                "type": "object",
                "required": ["from", "to"],
                "properties": {
                    "from": { "type": "string" },
                    "to": { "type": "string" },
                    "overwrite": { "type": "boolean" },
                    "conversation_id": { "type": "string" }
                }
            },
            "WorkspaceFileWriteBody": {
                "type": "object",
                "required": ["path", "content"],
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "create_only": { "type": "boolean" },
                    "update_only": { "type": "boolean" },
                    "create_directory": { "type": "boolean", "description": "为 true 时在 path 创建目录（content 须为空）" },
                    "parents": { "type": "boolean", "description": "create_directory 时递归创建父目录" }
                }
            },
            "WorkspaceFileWriteResponse": {
                "type": "object",
                "properties": {
                    "error": { "type": "string", "nullable": true }
                }
            },
            "WorkspaceFileDeleteResponse": {
                "type": "object",
                "properties": {
                    "error": { "type": "string", "nullable": true, "description": "单路径删除或整批校验失败时的错误信息；批量部分失败时为空（看 failed）" },
                    "deleted": { "type": "array", "items": { "type": "string" }, "description": "批量删除成功删除的相对路径（单路径删除时为空数组）" },
                    "failed": {
                        "type": "array",
                        "items": { "$ref": "#/components/schemas/WorkspaceFileDeleteFailure" },
                        "description": "批量删除失败项（非空时 error 为空）"
                    }
                }
            },
            "WorkspaceFileDeleteFailure": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "error": { "type": "string" }
                }
            },
            "WorkspaceDirCreateBody": {
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" },
                    "parents": { "type": "boolean" },
                    "delete": { "type": "boolean", "description": "为 true 时删除目录（须 confirm=true；非空目录须 recursive=true）" },
                    "confirm": { "type": "boolean" },
                    "recursive": { "type": "boolean" }
                }
            },
            "WorkspaceDirCreateResponse": {
                "type": "object",
                "properties": {
                    "error": { "type": "string", "nullable": true }
                }
            },
            "WorkspaceDirDeleteResponse": {
                "type": "object",
                "properties": {
                    "error": { "type": "string", "nullable": true }
                }
            },
            "TaskItem": {
                "type": "object",
                "required": ["id", "title", "done"],
                "properties": {
                    "id": { "type": "string" },
                    "title": { "type": "string" },
                    "done": { "type": "boolean" }
                }
            },
            "TasksData": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "nullable": true },
                    "updated_at": { "type": "string", "nullable": true },
                    "items": {
                        "type": "array",
                        "items": { "$ref": "#/components/schemas/TaskItem" }
                    }
                }
            },
    })
}

pub(super) fn openapi_components_value() -> Value {
    let schemas_merged = merge_component_objects(&[
        openapi_components_schemas_from_contract(),
        openapi_components_schemas_chat_core(),
        openapi_components_schemas_workspace_tasks_config(),
        openapi_components_schemas_tool_jobs(),
        openapi_components_user_data::openapi_components_schemas_user_data(),
    ]);
    let Value::Object(sec_root) = openapi_components_security_schemes() else {
        panic!("openapi security fragment must be a JSON object");
    };
    let mut root = sec_root;
    root.insert("schemas".to_string(), schemas_merged);
    Value::Object(root)
}

fn openapi_components_schemas_tool_jobs() -> Value {
    json!({
        "ToolJobStatusResponseBody": {
            "type": "object",
            "required": ["tool_job_id", "status", "workspace_changed", "result_version"],
            "properties": {
                "tool_job_id": { "type": "string" },
                "status": {
                    "type": "string",
                    "enum": ["queued", "running", "succeeded", "failed", "cancelled", "timed_out"]
                },
                "exit_code": { "type": "integer", "nullable": true },
                "stdout": { "type": "string", "nullable": true },
                "stderr": { "type": "string", "nullable": true },
                "summary": { "type": "string", "nullable": true },
                "error_code": { "type": "string", "nullable": true },
                "failure_category": { "type": "string", "nullable": true },
                "workspace_changed": { "type": "boolean" },
                "result_version": { "type": "integer" }
            }
        },
        "ToolJobCancelResponseBody": {
            "type": "object",
            "required": ["tool_job_id", "status"],
            "properties": {
                "tool_job_id": { "type": "string" },
                "status": { "type": "string" }
            }
        }
    })
}
