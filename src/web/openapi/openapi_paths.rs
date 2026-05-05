//! OpenAPI `paths` 对象（由 `openapi::build_openapi_spec` 组装）。
//!
//! 拆成多段 `json!`，降低单函数 `nloc`（`fn-nloc` 棘轮）；运行时合并为单一 object。

use serde_json::{Map, Value, json};

fn merge_path_fragments(fragments: &[Value]) -> Value {
    let mut map = Map::new();
    for fragment in fragments {
        let Value::Object(o) = fragment else {
            panic!("openapi path fragment must be a JSON object");
        };
        for (k, v) in o {
            if map.insert(k.clone(), v.clone()).is_some() {
                panic!("duplicate OpenAPI path key: {k}");
            }
        }
    }
    Value::Object(map)
}

fn openapi_paths_fragment_system() -> Value {
    json!({
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
                "summary": "CSR 展示开关（Markdown、助手展示过滤；受 CM_WEB_* 环境变量影响）",
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
    })
}

fn openapi_paths_fragment_chat_core() -> Value {
    json!({
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
                "description": "响应 `Content-Type: text/event-stream`；成功时响应头可含 `x-conversation-id`。事件载荷见 `docs/SSE协议.md`。",
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
    })
}

fn openapi_paths_fragment_chat_async() -> Value {
    json!({
        "/chat/async": {
            "post": {
                "tags": ["chat"],
                "summary": "异步非流式对话（轮询 / Webhook）",
                "description": "立即返回 job_id；执行与 POST /chat 相同 JSON 队列路径。状态仅存本进程内存，serve 重启丢失。需工具审批请用 POST /chat/stream。",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ChatAsyncRequestBody" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "已接受",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ChatAsyncSubmitResponseBody" }
                            }
                        }
                    },
                    "4XX": { "description": "参数或业务错误", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ApiError" } } } },
                    "503": { "description": "队列满（QUEUE_FULL）" }
                }
            }
        },
        "/chat/jobs/{job_id}": {
            "get": {
                "tags": ["chat"],
                "summary": "查询异步对话任务状态",
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
                        "description": "任务状态",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ChatJobStatusResponseBody" }
                            }
                        }
                    },
                    "404": { "description": "未知 job_id（UNKNOWN_JOB）" }
                }
            }
        }
    })
}

fn openapi_paths_fragment_chat_extras() -> Value {
    json!({
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
                "description": "供 Web 刷新后与 `conversation_id` 对齐；不含长期记忆/变更集/首轮工作区画像注入；404 表示不存在或已过期。",
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
    })
}

fn openapi_paths_fragment_workspace_list() -> Value {
    json!({
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
                "summary": "工作区选目录占位（始终 path=null；请在侧栏手动填写路径）",
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
    })
}

fn openapi_paths_fragment_workspace_rest() -> Value {
    json!({
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
        },
    })
}

pub(super) fn openapi_paths_value() -> Value {
    merge_path_fragments(&[
        openapi_paths_fragment_system(),
        openapi_paths_fragment_chat_core(),
        openapi_paths_fragment_chat_async(),
        openapi_paths_fragment_chat_extras(),
        openapi_paths_fragment_workspace_list(),
        openapi_paths_fragment_workspace_rest(),
    ])
}
