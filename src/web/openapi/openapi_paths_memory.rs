//! OpenAPI `paths` 片段：长期记忆管理（`/memory/*`）。
//!
//! 契约见 `docs/design/memory_management_api.md` §3。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_memory() -> Value {
    json!({
        "/memory/list": {
            "get": {
                "tags": ["memory"],
                "summary": "按 scope 投影长期记忆列表（只读）",
                "description": "scope 即 `conversation_id`；响应不含 embedding 与内部 source_role/source_kind。空 scope 返回 200 空列表。长期记忆未启用时 503 LONG_TERM_MEMORY_DISABLED（响应体含 enabled:false）。",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "conversation_id",
                        "in": "query",
                        "required": true,
                        "schema": { "type": "string" },
                        "description": "作用域；仅允许字母、数字、- _ . :，长度上限 128"
                    },
                    {
                        "name": "limit",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "integer", "format": "int32", "minimum": 1, "maximum": 200, "default": 50 },
                        "description": "每页条数；clamp 到 1..=200"
                    },
                    {
                        "name": "offset",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "integer", "format": "int32", "minimum": 0, "default": 0 }
                    },
                    {
                        "name": "kind",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "string", "enum": ["turn", "explicit", "experience", "other"] }
                    },
                    {
                        "name": "tag",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "string" },
                        "description": "命中 tags_json 中的标签"
                    },
                    {
                        "name": "q",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "string" },
                        "description": "chunk_text 子串（非 FTS）"
                    },
                    {
                        "name": "sort",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "string", "enum": ["created_at_desc", "created_at_asc"], "default": "created_at_desc" }
                    }
                ],
                "responses": {
                    "200": {
                        "description": "投影列表",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/MemoryListResponse" }
                            }
                        }
                    },
                    "400": { "description": "INVALID_CONVERSATION_ID / INVALID_MEMORY_KIND" },
                    "503": { "description": "LONG_TERM_MEMORY_DISABLED（enabled=false）" }
                }
            }
        },
        "/memory/{id}": {
            "delete": {
                "tags": ["memory"],
                "summary": "按 id 删除长期记忆（幂等）",
                "description": "行不存在 / 已过期 / 不属于该 conversation_id 均返回 204（不泄露 id 是否存在）。",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "id",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "integer", "format": "int64" }
                    },
                    {
                        "name": "conversation_id",
                        "in": "query",
                        "required": true,
                        "schema": { "type": "string" },
                        "description": "作用域守卫（必填）"
                    }
                ],
                "responses": {
                    "204": { "description": "已删除（或本就不存在 / 不属于该 scope）" },
                    "400": { "description": "INVALID_CONVERSATION_ID / INVALID_MEMORY_ID" },
                    "503": { "description": "LONG_TERM_MEMORY_DISABLED（enabled=false）" }
                }
            }
        }
    })
}
