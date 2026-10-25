//! OpenAPI `paths` 片段：权限等级只读投影（`/permission`）。
//!
//! 契约见 `docs/design/permission_levels.md` §16.2（响应形状由
//! `src/cm_api_contract/permission.rs` 的 `schemars` 定义生成 `PermissionView`）。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_permission() -> Value {
    json!({
        "/permission": {
            "get": {
                "tags": ["system"],
                "summary": "权限等级与六轴投影（只读策略投影）",
                "description": "客户端等级选择器的数据源：`available` 是选项集（全部 ≤ 上限，并已扣除角色上限与会话模式钳制），`clamped_by` 与 `ceiling_source` 解释「为什么本机不能选 L4」。刻意不提供写上限的接口（设计 §16.3）。",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "agent_role",
                        "in": "query",
                        "required": false,
                        "description": "即将使用的命名角色 id（决定角色上限 max_permission_level）；省略时按默认角色计算，未知角色返回 400 INVALID_AGENT_ROLE",
                        "schema": { "type": "string" }
                    },
                    {
                        "name": "conversation_id",
                        "in": "query",
                        "required": false,
                        "description": "可选会话 id：取该会话持久化的 agent_role 与 session_mode；不带时只返回静态投影",
                        "schema": { "type": "string" }
                    }
                ],
                "responses": {
                    "200": {
                        "description": "权限投影",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/PermissionView" }
                            }
                        }
                    },
                    "400": { "description": "agent_role / conversation_id 非法" },
                    "404": { "description": "会话不存在或已过期" }
                }
            }
        },
    })
}
