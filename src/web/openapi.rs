//! `GET /openapi.json`：OpenAPI 3.0 机器可读契约（与 `server.rs` 路由对齐；**不**替代 `docs/SSE_PROTOCOL.md` 对 SSE 行级语义的说明）。

use serde_json::{Value, json};

/// 构建与当前 `serve` 路由表一致的 OpenAPI 文档（不含静态 `/`、`/uploads` 文件服务细节）。
pub fn build_openapi_spec() -> Value {
    let version = env!("CARGO_PKG_VERSION");
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "CrabMate Web API",
            "version": version,
            "description": concat!(
                "CrabMate `serve` 模式的 HTTP 契约摘要。\n\n",
                "- **鉴权**：若进程配置了 `AGENT_WEB_API_BEARER_TOKEN`（或等价 TOML），下列标记为需鉴权的路径须在请求头携带 **`Authorization: Bearer <token>`** 或 **`X-API-Key: <token>`**（与配置值为**同一密钥**，二选一即可）；未配置时这些路径亦可匿名访问（部署时须自行评估风险）。\n",
                "- **SSE**：`POST /chat/stream` 返回 `text/event-stream`；控制面 JSON 与错误码见仓库 `docs/SSE_PROTOCOL.md`，本 OpenAPI 仅作入口说明。\n",
                "- **上传**：`POST /upload` 使用 `multipart/form-data`。"
            )
        },
        "tags": [
            { "name": "chat", "description": "对话与流式 SSE" },
            { "name": "workspace", "description": "工作区浏览与文件" },
            { "name": "system", "description": "健康检查与状态" },
            { "name": "tasks", "description": "进程内任务清单" },
            { "name": "config", "description": "配置热重载" },
            { "name": "uploads", "description": "上传与删除" }
        ],
        "paths": {
            "/openapi.json": {
                "get": {
                    "tags": ["system"],
                    "summary": "OpenAPI 本文档",
                    "responses": {
                        "200": {
                            "description": "OpenAPI 3.0 JSON",
                            "content": { "application/json": { "schema": { "type": "object" } } }
                        }
                    }
                }
            },
            "/health": {
                "get": {
                    "tags": ["system"],
                    "summary": "健康检查（依赖、可选 LLM models 探活等）",
                    "responses": {
                        "200": {
                            "description": "健康报告 JSON",
                            "content": { "application/json": { "schema": { "type": "object" } } }
                        }
                    }
                }
            },
            "/status": {
                "get": {
                    "tags": ["system"],
                    "summary": "运行状态（模型、工具数、规划配置等）",
                    "responses": {
                        "200": {
                            "description": "状态 JSON",
                            "content": { "application/json": { "schema": { "type": "object" } } }
                        }
                    }
                }
            },
            "/web-ui": {
                "get": {
                    "tags": ["system"],
                    "summary": "CSR 展示开关（Markdown、助手展示过滤；受 AGENT_WEB_* 环境变量影响）",
                    "responses": {
                        "200": {
                            "description": "Web UI 配置 JSON",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/WebUiConfigResponse" }
                                }
                            }
                        }
                    }
                }
            },
            "/chat": {
                "post": {
                    "tags": ["chat"],
                    "summary": "非流式对话",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ChatRequestBody" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "助手回复",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ChatResponseBody" }
                                }
                            }
                        },
                        "4XX": { "description": "业务或参数错误", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ApiError" } } } },
                        "5XX": { "description": "服务器错误" }
                    }
                }
            },
            "/chat/stream": {
                "post": {
                    "tags": ["chat"],
                    "summary": "SSE 流式对话",
                    "description": "响应 `Content-Type: text/event-stream`；成功时响应头可含 `x-conversation-id`。事件载荷见 `docs/SSE_PROTOCOL.md`。",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ChatRequestBody" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "SSE 字节流",
                            "content": { "text/event-stream": { "schema": { "type": "string", "format": "binary" } } }
                        },
                        "4XX": { "description": "错误（部分场景仍为 SSE 控制面 `error` 事件）" },
                        "5XX": { "description": "服务器错误" }
                    }
                }
            },
            "/chat/approval": {
                "post": {
                    "tags": ["chat"],
                    "summary": "工具/HTTP 审批决策",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ChatApprovalRequestBody" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "审批结果",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ChatApprovalResponseBody" }
                                }
                            }
                        }
                    }
                }
            },
            "/chat/branch": {
                "post": {
                    "tags": ["chat"],
                    "summary": "会话分叉截断（持久化会话）",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ChatBranchRequestBody" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "截断结果",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ChatBranchResponseBody" }
                                }
                            }
                        }
                    }
                }
            },
            "/conversation/messages": {
                "get": {
                    "tags": ["chat"],
                    "summary": "只读拉取已持久化会话消息与 revision",
                    "description": "供 Web 刷新后与 `conversation_id` 对齐；不含长期记忆/变更集注入；404 表示不存在或已过期。",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "parameters": [
                        {
                            "name": "conversation_id",
                            "in": "query",
                            "required": true,
                            "schema": { "type": "string" }
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "会话快照",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ConversationMessagesResponseBody" }
                                }
                            }
                        },
                        "400": { "description": "参数错误" },
                        "404": { "description": "会话不存在或已过期" }
                    }
                }
            },
            "/upload": {
                "post": {
                    "tags": ["uploads"],
                    "summary": "multipart 文件上传",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "multipart/form-data": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "file": { "type": "string", "format": "binary" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "上传结果",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/UploadResponseBody" }
                                }
                            }
                        }
                    }
                }
            },
            "/uploads/delete": {
                "post": {
                    "tags": ["uploads"],
                    "summary": "按 URL 删除已上传文件",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/DeleteUploadsBody" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "删除结果",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/DeleteUploadsResponseBody" }
                                }
                            }
                        }
                    }
                }
            },
            "/workspace": {
                "get": {
                    "tags": ["workspace"],
                    "summary": "列出工作区目录项",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "parameters": [
                        {
                            "name": "path",
                            "in": "query",
                            "required": false,
                            "schema": { "type": "string" },
                            "description": "相对子路径，可选"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "目录列表",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/WorkspaceResponse" }
                                }
                            }
                        }
                    }
                },
                "post": {
                    "tags": ["workspace"],
                    "summary": "设置当前 Web 工作区根",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/WorkspaceSetBody" }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "设置结果（JSON，形状与实现一致）" }
                    }
                }
            },
            "/workspace/pick": {
                "get": {
                    "tags": ["workspace"],
                    "summary": "服务端本机原生选目录（图形环境）",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "所选路径或 null",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/WorkspacePickResponse" }
                                }
                            }
                        }
                    }
                }
            },
            "/workspace/search": {
                "post": {
                    "tags": ["workspace"],
                    "summary": "工作区内搜索",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/WorkspaceSearchBody" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "搜索结果文本",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/WorkspaceSearchResponse" }
                                }
                            }
                        }
                    }
                }
            },
            "/workspace/file": {
                "get": {
                    "tags": ["workspace"],
                    "summary": "读取工作区内文本文件（有大小上限）",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "parameters": [
                        {
                            "name": "path",
                            "in": "query",
                            "required": true,
                            "schema": { "type": "string" }
                        },
                        {
                            "name": "encoding",
                            "in": "query",
                            "required": false,
                            "schema": { "type": "string" },
                            "description": "如 utf-8、gb18030、auto 等，与 `read_file` 工具一致"
                        }
                    ],
                    "responses": {
                        "200": { "description": "文件正文或 JSON 包装（与实现一致）" },
                        "4XX": { "description": "路径或编码错误" }
                    }
                },
                "post": {
                    "tags": ["workspace"],
                    "summary": "写入工作区文件",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/WorkspaceFileWriteBody" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "写入结果",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/WorkspaceFileWriteResponse" }
                                }
                            }
                        }
                    }
                },
                "delete": {
                    "tags": ["workspace"],
                    "summary": "删除工作区文件",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "parameters": [
                        {
                            "name": "path",
                            "in": "query",
                            "required": true,
                            "schema": { "type": "string" }
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "删除结果",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/WorkspaceFileDeleteResponse" }
                                }
                            }
                        }
                    }
                }
            },
            "/workspace/profile": {
                "get": {
                    "tags": ["workspace"],
                    "summary": "项目画像 Markdown",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "画像",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/WorkspaceProfileResponse" }
                                }
                            }
                        }
                    }
                }
            },
            "/workspace/changelog": {
                "get": {
                    "tags": ["workspace"],
                    "summary": "本会话工作区变更集 Markdown",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "parameters": [
                        {
                            "name": "conversation_id",
                            "in": "query",
                            "required": false,
                            "schema": { "type": "string" }
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "变更集",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/WorkspaceChangelogResponse" }
                                }
                            }
                        }
                    }
                }
            },
            "/tasks": {
                "get": {
                    "tags": ["tasks"],
                    "summary": "读取当前工作区任务清单（进程内存）",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "任务数据",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/TasksData" }
                                }
                            }
                        }
                    }
                },
                "post": {
                    "tags": ["tasks"],
                    "summary": "保存当前工作区任务清单",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/TasksData" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "回显保存后的数据",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/TasksData" }
                                }
                            }
                        }
                    }
                }
            },
            "/config/reload": {
                "post": {
                    "tags": ["config"],
                    "summary": "热重载 AgentConfig（不含部分字段，见 CONFIGURATION.md）",
                    "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": { "type": "object" },
                                "example": {}
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "重载结果",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ConfigReloadResponseBody" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "description": "与 `[agent].web_api_bearer_token` / `AGENT_WEB_API_BEARER_TOKEN` 一致；未启用服务端密钥时可为空。"
                },
                "apiKeyAuth": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-API-Key",
                    "description": "与 `web_api_bearer_token` 相同密钥；与 Bearer 二选一即可（常见于 Dify / Open WebUI 类网关习惯）。"
                }
            },
            "schemas": {
                "ClientLlmBody": {
                    "type": "object",
                    "properties": {
                        "api_base": { "type": "string" },
                        "model": { "type": "string" },
                        "api_key": { "type": "string", "description": "浏览器侧覆盖，勿记录到服务端日志" }
                    }
                },
                "WebUiConfigResponse": {
                    "type": "object",
                    "required": ["markdown_render", "apply_assistant_display_filters"],
                    "properties": {
                        "markdown_render": {
                            "type": "boolean",
                            "description": "为 false 时 CSR 跳过聊天气泡 Markdown（纯文本 HTML 转义）；由环境变量 AGENT_WEB_DISABLE_MARKDOWN 控制"
                        },
                        "apply_assistant_display_filters": {
                            "type": "boolean",
                            "description": "为 false 时不对助手消息做展示过滤（agent_reply_plan 剥离、内联思维链拆分等），且分阶段无工具规划轮可向浏览器 SSE 流式下发原文；为 true（默认）时对该轮做门控：规划 JSON 为 no_task 则整轮不下发，否则仅不下发 assistant_answer_phase 信封之前的流式增量。由环境变量 AGENT_WEB_RAW_ASSISTANT_OUTPUT 控制"
                        }
                    }
                },
                "ChatRequestBody": {
                    "type": "object",
                    "required": ["message"],
                    "properties": {
                        "message": { "type": "string" },
                        "conversation_id": { "type": "string" },
                        "agent_role": {
                            "type": "string",
                            "description": "Named role id; new session seeds first system; existing session refreshes first system if changed. See docs/CONFIGURATION.md § multi-role."
                        },
                        "approval_session_id": { "type": "string" },
                        "temperature": { "type": "number", "format": "double" },
                        "seed": { "type": "integer", "format": "int64" },
                        "seed_policy": { "type": "string", "description": "如 omit / none" },
                        "client_llm": { "$ref": "#/components/schemas/ClientLlmBody" },
                        "client_sse_protocol": {
                            "type": "integer",
                            "format": "int32",
                            "description": "可选；客户端 SSE 控制面版本，须 ≤ 服务端。大于服务端时 400（SSE_CLIENT_TOO_NEW）。见 docs/SSE_PROTOCOL.md"
                        },
                        "image_urls": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "可选；须为先前 POST /upload 返回的 /uploads/... 相对路径（最多 6 条）；与 message 一并组装多模态 user 消息"
                        },
                        "clarify_questionnaire_answers": {
                            "type": "object",
                            "description": "可选；回应 SSE clarification_questionnaire；可与空 message 单独提交",
                            "properties": {
                                "questionnaire_id": {
                                    "type": "string",
                                    "description": "与 SSE 中 questionnaire_id 一致"
                                },
                                "answers": {
                                    "type": "object",
                                    "additionalProperties": true,
                                    "description": "键为题 id；值多为字符串"
                                }
                            }
                        }
                    }
                },
                "ChatResponseBody": {
                    "type": "object",
                    "properties": {
                        "reply": { "type": "string" },
                        "conversation_id": { "type": "string" },
                        "conversation_revision": { "type": "integer", "format": "int64", "nullable": true }
                    }
                },
                "ChatApprovalRequestBody": {
                    "type": "object",
                    "required": ["approval_session_id", "decision"],
                    "properties": {
                        "approval_session_id": { "type": "string" },
                        "decision": { "type": "string" }
                    }
                },
                "ChatApprovalResponseBody": {
                    "type": "object",
                    "properties": {
                        "ok": { "type": "boolean" }
                    }
                },
                "ChatBranchRequestBody": {
                    "type": "object",
                    "required": ["conversation_id", "before_user_ordinal", "expected_revision"],
                    "properties": {
                        "conversation_id": { "type": "string" },
                        "before_user_ordinal": { "type": "integer", "format": "int64" },
                        "expected_revision": { "type": "integer", "format": "int64" }
                    }
                },
                "ChatBranchResponseBody": {
                    "type": "object",
                    "properties": {
                        "ok": { "type": "boolean" },
                        "revision": { "type": "integer", "format": "int64" }
                    }
                },
                "ConversationMessagesResponseBody": {
                    "type": "object",
                    "required": ["conversation_id", "revision", "messages"],
                    "properties": {
                        "conversation_id": { "type": "string" },
                        "revision": { "type": "integer", "format": "int64" },
                        "active_agent_role": { "type": "string", "description": "非空时与配置中 agent_roles 对齐" },
                        "messages": {
                            "type": "array",
                            "description": "OpenAI 兼容 Message 数组（已剔除长期记忆/变更集注入）",
                            "items": { "type": "object", "additionalProperties": true }
                        }
                    }
                },
                "UploadedFileInfo": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string" },
                        "filename": { "type": "string" },
                        "mime": { "type": "string" },
                        "size": { "type": "integer", "format": "int64" }
                    }
                },
                "UploadResponseBody": {
                    "type": "object",
                    "properties": {
                        "files": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/UploadedFileInfo" }
                        }
                    }
                },
                "DeleteUploadsBody": {
                    "type": "object",
                    "required": ["urls"],
                    "properties": {
                        "urls": { "type": "array", "items": { "type": "string" } }
                    }
                },
                "DeleteUploadsResponseBody": {
                    "type": "object",
                    "properties": {
                        "deleted": { "type": "array", "items": { "type": "string" } },
                        "skipped": { "type": "array", "items": { "type": "string" } }
                    }
                },
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
                "WorkspaceChangelogResponse": {
                    "type": "object",
                    "properties": {
                        "revision": { "type": "integer", "format": "int64" },
                        "markdown": { "type": "string" },
                        "error": { "type": "string", "nullable": true }
                    }
                },
                "WorkspaceFileWriteBody": {
                    "type": "object",
                    "required": ["path", "content"],
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" },
                        "create_only": { "type": "boolean" },
                        "update_only": { "type": "boolean" }
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
                "ConfigReloadResponseBody": {
                    "type": "object",
                    "properties": {
                        "ok": { "type": "boolean" },
                        "message": { "type": "string" }
                    }
                },
                "ApiError": {
                    "type": "object",
                    "properties": {
                        "code": { "type": "string" },
                        "message": { "type": "string" }
                    }
                }
            }
        }
    })
}

/// Axum handler：`application/json` OpenAPI 文档。
pub(crate) async fn openapi_json_handler() -> axum::Json<Value> {
    axum::Json(build_openapi_spec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_spec_has_core_paths_and_version() {
        let v = build_openapi_spec();
        assert_eq!(v["openapi"], "3.0.3");
        let paths = v["paths"].as_object().expect("paths object");
        assert!(paths.contains_key("/health"));
        assert!(paths.contains_key("/web-ui"));
        assert!(paths.contains_key("/chat/stream"));
        assert!(paths.contains_key("/conversation/messages"));
        assert!(paths.contains_key("/openapi.json"));
        assert!(v["components"]["securitySchemes"]["bearerAuth"].is_object());
        assert!(v["components"]["securitySchemes"]["apiKeyAuth"].is_object());
    }
}
