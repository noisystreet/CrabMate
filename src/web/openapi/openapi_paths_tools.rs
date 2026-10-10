//! OpenAPI `paths` 中工具管理端点片段（`/tools`、`/tools/plugins`、
//! `/tools/plugins/{file}`、`/tools/{tool_name}`、`/tools/{tool_name}/enabled`）。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_tools() -> Value {
    json!({
        "/tools": tools_list_path(),
        "/tools/plugins": tools_plugins_path(),
        "/tools/plugins/{file}": tool_plugin_detail_path(),
        "/tools/{tool_name}": tool_detail_path(),
        "/tools/{tool_name}/enabled": tool_enabled_path(),
    })
}

/// `GET /tools`：三源枚举只读清单。
fn tools_list_path() -> Value {
    json!({
        "get": {
            "tags": ["tools"],
            "summary": "枚举工具（内置 / 工作区动态 / MCP 三源只读清单）",
            "description": "只读投影；**绝不建立 MCP 会话**，MCP 源仅取已缓存会话快照与已配置 server。`counts` 为过滤后、分页前计数。",
            "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
            "parameters": [
                {
                    "name": "source",
                    "in": "query",
                    "required": false,
                    "description": "按来源过滤，可重复；取值 builtin / dynamic / mcp。非法取值 400 INVALID_TOOL_FILTER",
                    "schema": { "type": "array", "items": { "type": "string", "enum": ["builtin", "dynamic", "mcp"] } }
                },
                {
                    "name": "category",
                    "in": "query",
                    "required": false,
                    "description": "按分类过滤；取值 basic / development。非法取值 400 INVALID_TOOL_FILTER",
                    "schema": { "type": "string", "enum": ["basic", "development"] }
                },
                {
                    "name": "dev_tag",
                    "in": "query",
                    "required": false,
                    "description": "按开发标签过滤（仅内置工具有标签）",
                    "schema": { "type": "string" }
                },
                {
                    "name": "q",
                    "in": "query",
                    "required": false,
                    "description": "名称/描述大小写不敏感子串匹配",
                    "schema": { "type": "string" }
                },
                {
                    "name": "limit",
                    "in": "query",
                    "required": false,
                    "description": "分页大小，默认 50，clamp 到 1..=200；非数值 400 INVALID_TOOL_FILTER",
                    "schema": { "type": "integer", "minimum": 1, "maximum": 200, "default": 50 }
                },
                {
                    "name": "offset",
                    "in": "query",
                    "required": false,
                    "description": "分页偏移，默认 0；非数值 400 INVALID_TOOL_FILTER",
                    "schema": { "type": "integer", "minimum": 0, "default": 0 }
                }
            ],
            "responses": {
                "200": {
                    "description": "工具清单（含 total/limit/offset/has_more/counts）",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "tools": { "type": "array", "items": { "type": "object" } },
                                    "total": { "type": "integer" },
                                    "limit": { "type": "integer" },
                                    "offset": { "type": "integer" },
                                    "has_more": { "type": "boolean" },
                                    "counts": {
                                        "type": "object",
                                        "properties": {
                                            "builtin": { "type": "integer" },
                                            "dynamic": { "type": "integer" },
                                            "mcp": { "type": "integer" },
                                            "present": { "type": "integer" }
                                        }
                                    },
                                    "workspace_root": { "type": "string" }
                                }
                            }
                        }
                    }
                },
                "400": { "description": "非法过滤参数（INVALID_TOOL_FILTER）" }
            }
        }
    })
}

/// `GET /tools/plugins`：工作区动态工具文件只读清单。
fn tools_plugins_path() -> Value {
    json!({
        "get": {
            "tags": ["tools"],
            "summary": "列出工作区动态工具文件（`<workspace>/plugins/*.json`，只读）",
            "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
            "responses": {
                "200": {
                    "description": "动态工具文件清单（含校验失败原因与命令白名单判定）",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "plugins": { "type": "array", "items": { "type": "object" } },
                                    "total": { "type": "integer" }
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

/// `GET /tools/plugins/{file}`：工作区动态工具单文件只读详情。
fn tool_plugin_detail_path() -> Value {
    json!({
        "get": {
            "tags": ["tools"],
            "summary": "读取工作区单个动态工具文件（`<workspace>/plugins/{file}`，只读）",
            "description": "返回该文件的 `parameters` / `args` / `pass_args_json` 与校验结果；不做写/删。`file` 仅允许 `[A-Za-z0-9_-]+\\.json`（显式拒绝分隔符与 `..`）。",
            "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
            "parameters": [
                {
                    "name": "file",
                    "in": "path",
                    "required": true,
                    "description": "插件文件名，须匹配 `[A-Za-z0-9_-]+\\.json`",
                    "schema": { "type": "string" }
                }
            ],
            "responses": {
                "200": {
                    "description": "单文件详情（含校验结果与原始参数）",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "file": { "type": "string" },
                                    "name": { "type": "string" },
                                    "description": { "type": "string" },
                                    "valid": { "type": "boolean" },
                                    "error": { "type": "string" },
                                    "command_allowed": { "type": "boolean" },
                                    "parameters": { "type": "object" },
                                    "args": { "type": "array", "items": { "type": "string" } },
                                    "pass_args_json": { "type": "boolean" }
                                }
                            }
                        }
                    }
                },
                "400": { "description": "文件名不合法（INVALID_PLUGIN_DEFINITION）" },
                "404": { "description": "未找到动态工具文件（PLUGIN_NOT_FOUND）" }
            }
        }
    })
}

/// `GET /tools/{tool_name}`：单工具详情与策略投影。
fn tool_detail_path() -> Value {
    json!({
        "get": {
            "tags": ["tools"],
            "summary": "单工具详情与策略投影",
            "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
            "parameters": [
                {
                    "name": "tool_name",
                    "in": "path",
                    "required": true,
                    "schema": { "type": "string" }
                }
            ],
            "responses": {
                "200": {
                    "description": "工具详情（含 parameters 与 policy）",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "description": { "type": "string" },
                                    "category": { "type": "string" },
                                    "source": { "type": "string" },
                                    "dev_tags": { "type": "array", "items": { "type": "string" } },
                                    "present": { "type": "boolean" },
                                    "absent_reason": { "type": "string" },
                                    "parameters": { "type": "object" },
                                    "policy": {
                                        "type": "object",
                                        "properties": {
                                            "read_only": { "type": "boolean" },
                                            "write_effect": { "type": "boolean" },
                                            "parallel_readonly_batch_allowed": { "type": "boolean" },
                                            "sync_default_inline": { "type": "boolean" },
                                            "wall_timeout_secs": { "type": "integer" },
                                            "sub_agent_extra_allow": {
                                                "type": "array",
                                                "items": { "type": "string" },
                                                "description": "命中的 sub_agent_*_extra_tools 集合名"
                                            },
                                            "background_job_capable": { "type": "boolean" },
                                            "retry_eligible": { "type": "boolean" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "400": { "description": "工具名不合法（INVALID_TOOL_NAME）" },
                "404": { "description": "未注册的工具（TOOL_NOT_FOUND）" }
            }
        }
    })
}

/// `PUT /tools/{tool_name}/enabled`：启用/停用工具（写 `tool_overrides.json` 并就地生效）。
fn tool_enabled_path() -> Value {
    json!({
        "put": {
            "tags": ["tools"],
            "summary": "启用/停用工具（写 tool_overrides.json 并就地生效）",
            "description": "幂等：始终写入显式 bool。写入后立即更新运行中配置，无需重启；重启/reload 亦生效。`tool_overrides.json` 仅解除精确禁用集；若该名仍被 `disabled_tool_prefixes` 命中，响应 `present:false` 且带 `absent_reason`（不谎报生效）。",
            "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
            "parameters": [
                {
                    "name": "tool_name",
                    "in": "path",
                    "required": true,
                    "schema": { "type": "string" }
                }
            ],
            "requestBody": {
                "required": true,
                "content": {
                    "application/json": {
                        "schema": {
                            "type": "object",
                            "required": ["enabled"],
                            "properties": { "enabled": { "type": "boolean" } }
                        }
                    }
                }
            },
            "responses": {
                "200": {
                    "description": "已写入并生效",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "enabled": { "type": "boolean" },
                                    "source": { "type": "string" },
                                    "effective_after_reload": { "type": "boolean" },
                                    "present": { "type": "boolean" },
                                    "absent_reason": { "type": "string" }
                                }
                            }
                        }
                    }
                },
                "400": { "description": "工具名不合法（INVALID_TOOL_NAME）" },
                "404": { "description": "未注册的工具（TOOL_NOT_FOUND）" },
                "500": { "description": "写入 tool_overrides.json 失败（INTERNAL_ERROR）" }
            }
        }
    })
}
