//! OpenAPI `components/schemas`：`/user-data` 相关类型。

use serde_json::{Value, json};

pub(super) fn openapi_components_schemas_user_data() -> Value {
    json!({
            "UserPrefs": {
                "type": "object",
                "description": "prefs.json（非机密壳层偏好）"
            },
            "LlmOverridesFile": {
                "type": "object",
                "description": "llm_overrides.json"
            },
            "SecretsStatusResponse": {
                "type": "object",
                "properties": {
                    "client_llm": { "$ref": "#/components/schemas/SecretSlotStatus" },
                    "executor_llm": { "$ref": "#/components/schemas/SecretSlotStatus" },
                    "web_api_bearer": { "$ref": "#/components/schemas/SecretSlotStatus" }
                }
            },
            "SecretSlotStatus": {
                "type": "object",
                "properties": {
                    "set": { "type": "boolean" },
                    "suffix": { "type": "string", "nullable": true }
                }
            },
            "SecretWriteBody": {
                "type": "object",
                "properties": {
                    "api_key": { "type": "string" },
                    "token": { "type": "string" }
                }
            },
            "WebSessionsFile": {
                "type": "object",
                "properties": {
                    "schema_version": { "type": "integer" },
                    "sessions": { "type": "array", "items": { "type": "object" } },
                    "active_session_id": { "type": "string", "nullable": true }
                }
            },
            "PutWebSessionsBody": {
                "type": "object",
                "properties": {
                    "sessions": { "type": "array", "items": { "type": "object" } },
                    "active_session_id": { "type": "string", "nullable": true }
                }
            },
            "WorkspaceListEntry": {
                "type": "object",
                "properties": {
                    "hash": { "type": "string" },
                    "workspace_root": { "type": "string" }
                }
            },
    })
}
