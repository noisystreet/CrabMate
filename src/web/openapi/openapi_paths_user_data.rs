//! OpenAPI `/user-data/*` 路径片段。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_user_data() -> Value {
    json!({
        "/user-data/prefs": {
            "get": {
                "tags": ["user_data"],
                "summary": "读取本机用户偏好",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "responses": {
                    "200": {
                        "description": "prefs.json",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/UserPrefs" }
                            }
                        }
                    }
                }
            },
            "put": {
                "tags": ["user_data"],
                "summary": "写回本机用户偏好",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/UserPrefs" }
                        }
                    }
                },
                "responses": { "204": { "description": "已保存" } }
            }
        },
        "/user-data/llm-overrides": {
            "get": {
                "tags": ["user_data"],
                "summary": "读取 LLM 非机密覆盖",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "responses": {
                    "200": {
                        "description": "llm_overrides.json",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/LlmOverridesFile" }
                            }
                        }
                    }
                }
            },
            "put": {
                "tags": ["user_data"],
                "summary": "写回 LLM 非机密覆盖",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/LlmOverridesFile" }
                        }
                    }
                },
                "responses": { "204": { "description": "已保存" } }
            }
        },
        "/user-data/secrets/status": {
            "get": {
                "tags": ["user_data"],
                "summary": "密钥槽脱敏状态（无明文）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "responses": {
                    "200": {
                        "description": "各密钥槽是否已设置（LLM 槽来自系统钥匙串）",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/SecretsStatusResponse" }
                            }
                        }
                    }
                }
            }
        },
        "/user-data/secrets/client-llm": {
            "put": {
                "tags": ["user_data"],
                "summary": "写主模型 API Key 到系统钥匙串（仅写；无 GET 明文）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/SecretWriteBody" }
                        }
                    }
                },
                "responses": { "204": { "description": "已保存或已清除" } }
            }
        },
        "/user-data/secrets/github": {
            "put": {
                "tags": ["user_data"],
                "summary": "写 GitHub token（PAT / OAuth）到系统钥匙串账户 github（仅写；无 GET 明文）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/SecretWriteBody" }
                        }
                    }
                },
                "responses": { "204": { "description": "已保存或已清除（空 token）" } }
            },
            "delete": {
                "tags": ["user_data"],
                "summary": "清除钥匙串 GitHub token（账户 github）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "responses": { "204": { "description": "已清除" } }
            }
        },
        "/user-data/secrets/github-oauth-client-id": {
            "put": {
                "tags": ["user_data"],
                "summary": "写 GitHub App/OAuth Client ID 到钥匙串账户 github_oauth_client_id（body.token；无 GET 明文）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/SecretWriteBody" }
                        }
                    }
                },
                "responses": { "204": { "description": "已保存或已清除（空 token）" } }
            },
            "delete": {
                "tags": ["user_data"],
                "summary": "清除钥匙串 GitHub OAuth Client ID",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "responses": { "204": { "description": "已清除" } }
            }
        },
        "/user-data/workspaces/current/sessions": {
            "get": {
                "tags": ["user_data"],
                "summary": "当前工作区侧栏会话列表",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "responses": {
                    "200": {
                        "description": "web_sessions.json",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/WebSessionsFile" }
                            }
                        }
                    }
                }
            },
            "put": {
                "tags": ["user_data"],
                "summary": "写当前工作区侧栏会话",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/PutWebSessionsBody" }
                        }
                    }
                },
                "responses": { "204": { "description": "已保存" } }
            }
        },
        "/user-data/workspaces": {
            "get": {
                "tags": ["user_data"],
                "summary": "列举已分桶工作区",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "responses": {
                    "200": {
                        "description": "manifest 摘要列表",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "array",
                                    "items": { "$ref": "#/components/schemas/WorkspaceListEntry" }
                                }
                            }
                        }
                    }
                }
            }
        },
    })
}
