//! OpenAPI `paths` 片段：会话级运行态意图写入口（`POST /conversation/state`）。
//!
//! 契约见 `src/cm_api_contract/session_state.rs`（`ConversationStatePatch` / `ConversationStateView`）。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_conversation_state() -> Value {
    json!({
        "/conversation/state": {
            "post": {
                "tags": ["chat"],
                "summary": "写入会话级运行态意图（agent_role / session_mode / permission_level）",
                "description": "供 Client 在**空闲态**切换权限等级 / 会话模式 / 命名角色后立即落库：只改 `active_*` 意图列，不碰消息、不 bump revision（因此不会让进行中回合的落盘变成 CONVERSATION_CONFLICT）。三个字段至少给一个；未知取值 400；会话不存在 404（不做 upsert）。写入的是**意图**，生效仍按回合内的 min 链重算，故不构成提权通道。响应回带与 `GET /permission` 同一份 `PermissionView`。",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ConversationStatePatch" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "写入后的会话状态与权限投影",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ConversationStateView" }
                            }
                        }
                    },
                    "400": {
                        "description": "参数错误：INVALID_CONVERSATION_ID / INVALID_PERMISSION_LEVEL / INVALID_SESSION_MODE / INVALID_AGENT_ROLE / EMPTY_CONVERSATION_STATE_PATCH",
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ApiError" } } }
                    },
                    "404": { "description": "会话不存在或已过期（CONVERSATION_NOT_FOUND）" }
                }
            }
        },
    })
}
