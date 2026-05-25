//! OpenAPI `components` 对象（由 `openapi::build_openapi_spec` 组装）。
//!
//! 拆成多段 `json!` + `merge_component_objects`，降低单函数 `nloc`（`fn-nloc` 棘轮）；运行时合并为单一 object。

use serde_json::{Map, Value, json};

use super::openapi_components_user_data;

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
            "ClientLlmBody": {
                "type": "object",
                "properties": {
                    "api_base": { "type": "string" },
                    "model": { "type": "string" },
                    "api_key": { "type": "string", "description": "浏览器侧覆盖，勿记录到服务端日志" },
                    "llm_context_tokens": { "type": "integer", "format": "int64", "description": "可选；本回合上下文窗口 token 上限覆盖" },
                    "llm_thinking_mode": { "type": "string", "enum": ["server", "on", "off"], "description": "可选；本回合 thinking 策略：server 跟随服务端；on 开启（智谱 thinking enabled；DeepSeek 官方 api_base 时 thinking enabled + reasoning_effort high；Kimi k2.5 不发送 disabled）；off 关闭（智谱不写 thinking；DeepSeek 写 thinking disabled；Kimi k2.5 发送 thinking disabled）" }
                }
            },
            "ExecutorLlmBody": {
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
                        "description": "为 false 时 CSR 跳过聊天气泡 Markdown（纯文本 HTML 转义）；由环境变量 CM_WEB_DISABLE_MARKDOWN 控制"
                    },
                    "apply_assistant_display_filters": {
                        "type": "boolean",
                        "description": "为 false 时不对助手消息做展示过滤（agent_reply_plan 剥离、内联思维链拆分等），且分阶段无工具规划轮可向浏览器 SSE 流式下发原文；为 true（默认）时对该轮做门控：解析自正文+思维链的规划 JSON 为 no_task 则整轮 SSE 不下发且不写入会话 assistant 列表，否则仅不下发 assistant_answer_phase 信封之前的流式增量。由环境变量 CM_WEB_RAW_ASSISTANT_OUTPUT 控制"
                    }
                }
            },
    })
}

fn openapi_components_schemas_chat_request() -> Value {
    json!({
            "ChatRequestBody": {
                "type": "object",
                "required": ["message"],
                "properties": {
                    "message": { "type": "string" },
                    "conversation_id": { "type": "string" },
                    "agent_role": {
                        "type": "string",
                        "description": "Named role id; new session seeds first system; existing session refreshes first system if changed. See docs/配置说明.md § multi-role."
                    },
                    "approval_session_id": { "type": "string" },
                    "temperature": { "type": "number", "format": "double" },
                    "seed": { "type": "integer", "format": "int64" },
                    "seed_policy": { "type": "string", "description": "如 omit / none" },
                    "client_llm": { "$ref": "#/components/schemas/ClientLlmBody" },
                    "executor_llm": { "$ref": "#/components/schemas/ExecutorLlmBody" },
                    "execution_mode": {
                        "type": "string",
                        "description": "可选；本回合执行模式覆盖：rolling_planning 或 hierarchical"
                    },
                    "readonly_tool_ttl_cache_secs": {
                        "type": "integer",
                        "format": "int64",
                        "description": "可选；本回合覆盖只读 run_command 进程内 TTL 缓存秒数；0 关闭；上限 3600；省略则跟随服务端配置"
                    },
                    "client_sse_protocol": {
                        "type": "integer",
                        "format": "int32",
                        "description": "可选；客户端 SSE 控制面版本，须 ≤ 服务端。大于服务端时 400（SSE_CLIENT_TOO_NEW）。见 docs/SSE协议.md"
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
    })
}

fn openapi_components_schemas_chat_response_approval_branch() -> Value {
    json!({
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
            "ChatAsyncRequestBody": {
                "allOf": [
                    { "$ref": "#/components/schemas/ChatRequestBody" },
                    {
                        "type": "object",
                        "properties": {
                            "webhook_url": {
                                "type": "string",
                                "description": "可选；http/https；任务 completed/failed 后 POST JSON 回调"
                            },
                            "webhook_secret": {
                                "type": "string",
                                "description": "可选；随回调发送请求头 X-Crabmate-Webhook-Secret（最多 256 字符）"
                            }
                        }
                    }
                ]
            },
            "ChatAsyncSubmitResponseBody": {
                "type": "object",
                "required": ["job_id", "status", "conversation_id"],
                "properties": {
                    "job_id": { "type": "integer", "format": "int64" },
                    "status": { "type": "string", "description": "初始为 pending" },
                    "conversation_id": { "type": "string" }
                }
            },
            "ChatJobStatusResponseBody": {
                "type": "object",
                "required": ["job_id", "status", "conversation_id"],
                "properties": {
                    "job_id": { "type": "integer", "format": "int64" },
                    "status": { "type": "string", "enum": ["pending", "running", "completed", "failed"] },
                    "conversation_id": { "type": "string" },
                    "reply": { "type": "string", "nullable": true },
                    "conversation_revision": { "type": "integer", "format": "int64", "nullable": true },
                    "error": { "$ref": "#/components/schemas/ApiError", "nullable": true }
                }
            },
    })
}

fn openapi_components_schemas_chat_messages_uploads() -> Value {
    json!({
            "ConversationMessagesResponseBody": {
                "type": "object",
                "required": ["conversation_id", "revision", "messages"],
                "properties": {
                    "conversation_id": { "type": "string" },
                    "revision": { "type": "integer", "format": "int64" },
                    "active_agent_role": { "type": "string", "description": "非空时与配置中 agent_roles 对齐" },
                    "tiktoken_prompt_tokens": {
                        "type": "object",
                        "description": "与会话落盘消息经出站规则后的 tiktoken prompt 粗估；不含 tools JSON",
                        "properties": {
                            "prompt_tokens": { "type": "integer", "format": "int64" },
                            "tiktoken_model": { "type": "string" }
                        }
                    },
                    "messages": {
                        "type": "array",
                        "description": "OpenAI 兼容 Message 数组（已剔除长期记忆/变更集注入、普通 system 系统提示与 UI 分隔；保留 name=crabmate_timeline 时间线旁注）",
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
    })
}

fn openapi_components_schemas_chat_core() -> Value {
    merge_component_objects(&[
        openapi_components_schemas_chat_llm_webui(),
        openapi_components_schemas_chat_request(),
        openapi_components_schemas_chat_response_approval_branch(),
        openapi_components_schemas_chat_messages_uploads(),
    ])
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
            "SessionConversationStoreRequestBody": {
                "type": "object",
                "required": ["sqlite"],
                "properties": {
                    "sqlite": {
                        "type": "boolean",
                        "description": "true：使用配置中的 SQLite 路径；false：本进程改用内存会话存储"
                    }
                }
            },
            "ApiError": {
                "type": "object",
                "properties": {
                    "code": { "type": "string" },
                    "message": { "type": "string" },
                    "reason_code": {
                        "type": "string",
                        "nullable": true,
                        "description": "When present: truncated internal detail for `INTERNAL_ERROR` on `POST /chat` JSON only; SSE may use `reason_code` more broadly (see docs/SSE协议.md)"
                    }
                }
            },
    })
}

pub(super) fn openapi_components_value() -> Value {
    let schemas_merged = merge_component_objects(&[
        openapi_components_schemas_chat_core(),
        openapi_components_schemas_workspace_tasks_config(),
        openapi_components_user_data::openapi_components_schemas_user_data(),
    ]);
    let Value::Object(sec_root) = openapi_components_security_schemes() else {
        panic!("openapi security fragment must be a JSON object");
    };
    let mut root = sec_root;
    root.insert("schemas".to_string(), schemas_merged);
    Value::Object(root)
}
