//! OpenAPI `/user-data` MCP 与 Web API Bearer 写路径（与 `routes/user_data` 对齐）。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_user_data_mcp() -> Value {
    json!({
        "/user-data/secrets/web-api-bearer": {
            "put": {
                "tags": ["user_data"],
                "summary": "写入或清除本机 Web API Bearer（钥匙串；body.token 或 api_key）",
                "security": super::openapi_helpers::protected_security(),
                "requestBody": {
                    "required": true,
                    "content": super::openapi_helpers::json_content(json!({
                                "type": "object",
                                "properties": {
                                    "token": { "type": "string" },
                                    "api_key": { "type": "string" }
                                }
                            }))
                },
                "responses": { "204": { "description": "已写入或已清除" } }
            }
        },
        "/user-data/mcp-servers": {
            "get": {
                "tags": ["user_data"],
                "summary": "读取本机 MCP 服务器清单（脱敏）",
                "security": super::openapi_helpers::protected_security(),
                "responses": {
                    "200": {
                        "description": "mcp_servers 公共视图",
                        "content": super::openapi_helpers::json_content(json!({ "type": "object" }))
                    }
                }
            },
            "put": {
                "tags": ["user_data"],
                "summary": "写回 MCP 服务器清单（CrabMate 形状或含 mcpServers 的导入 JSON）",
                "security": super::openapi_helpers::protected_security(),
                "requestBody": {
                    "required": true,
                    "content": super::openapi_helpers::json_content(json!({ "type": "object" }))
                },
                "responses": {
                    "204": { "description": "已保存" },
                    "400": { "description": "JSON 无效" }
                }
            }
        },
        "/user-data/mcp-servers/import": {
            "post": {
                "tags": ["user_data"],
                "summary": "追加导入 MCP 配置 JSON（对象或 JSON 字符串）",
                "security": super::openapi_helpers::protected_security(),
                "requestBody": {
                    "required": true,
                    "content": super::openapi_helpers::json_content(json!({}))
                },
                "responses": {
                    "200": {
                        "description": "导入结果",
                        "content": super::openapi_helpers::json_content(json!({ "type": "object" }))
                    },
                    "400": { "description": "JSON 无效" }
                }
            }
        },
        "/user-data/mcp-servers/status": {
            "get": {
                "tags": ["user_data"],
                "summary": "MCP 服务器运行时状态（不探测）",
                "security": super::openapi_helpers::protected_security(),
                "responses": {
                    "200": {
                        "description": "global_enabled / servers[]",
                        "content": super::openapi_helpers::json_content(json!({ "type": "object" }))
                    }
                }
            }
        },
        "/user-data/mcp-servers/probe-all": {
            "post": {
                "tags": ["user_data"],
                "summary": "探测所有已启用的 MCP 服务器",
                "security": super::openapi_helpers::protected_security(),
                "responses": {
                    "200": {
                        "description": "各服务器探测结果",
                        "content": super::openapi_helpers::json_content(json!({ "type": "array", "items": { "type": "object" } }))
                    }
                }
            }
        },
        "/user-data/mcp-servers/{id}/remote-auth": {
            "put": {
                "tags": ["user_data"],
                "summary": "为远程（url）MCP 服务器设置或清除 Bearer",
                "security": super::openapi_helpers::protected_security(),
                "parameters": [{
                    "name": "id",
                    "in": "path",
                    "required": true,
                    "schema": { "type": "string" }
                }],
                "requestBody": {
                    "required": true,
                    "content": super::openapi_helpers::json_content(json!({
                                "type": "object",
                                "properties": { "bearer_token": { "type": "string" } }
                            }))
                },
                "responses": {
                    "204": { "description": "已写入或已清除" },
                    "400": { "description": "非远程服务器或 id 无效" },
                    "404": { "description": "未找到 MCP 服务器" }
                }
            }
        },
        "/user-data/mcp-servers/{id}/probe": {
            "post": {
                "tags": ["user_data"],
                "summary": "探测单个 MCP 服务器",
                "security": super::openapi_helpers::protected_security(),
                "parameters": [{
                    "name": "id",
                    "in": "path",
                    "required": true,
                    "schema": { "type": "string" }
                }],
                "responses": {
                    "200": {
                        "description": "探测结果",
                        "content": super::openapi_helpers::json_content(json!({ "type": "object" }))
                    },
                    "404": { "description": "未找到 MCP 服务器" }
                }
            }
        }
    })
}
