//! `/workspace/file/raw` OpenAPI 片段（从 `openapi_paths.rs` 拆出以控制文件行数）。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_workspace_file_raw() -> Value {
    json!({
        "/workspace/file/raw": {
            "get": {
                "tags": ["workspace"],
                "summary": "读取工作区内常见图片原始字节（png/jpg/jpeg/webp/gif；上限 8 MiB）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "path",
                        "in": "query",
                        "required": true,
                        "schema": { "type": "string" },
                        "description": "工作区相对路径；禁止 `..`；不含 svg"
                    }
                ],
                "responses": {
                    "200": {
                        "description": "图片字节",
                        "content": {
                            "image/png": { "schema": { "type": "string", "format": "binary" } },
                            "image/jpeg": { "schema": { "type": "string", "format": "binary" } },
                            "image/webp": { "schema": { "type": "string", "format": "binary" } },
                            "image/gif": { "schema": { "type": "string", "format": "binary" } }
                        }
                    },
                    "4XX": { "description": "路径、类型或大小错误（JSON ApiError）" }
                }
            },
            "put": {
                "tags": ["workspace"],
                "summary": "写入工作区文件原始字节（任意类型；上限 16 MiB；与 POST /workspace/file 的 JSON content 上限一致）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "path",
                        "in": "query",
                        "required": true,
                        "schema": { "type": "string" },
                        "description": "工作区相对路径；禁止 `..`"
                    },
                    {
                        "name": "create_only",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "boolean" },
                        "description": "为 true 时若目标已存在则 409 WORKSPACE_FILE_EXISTS"
                    },
                    {
                        "name": "update_only",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "boolean" },
                        "description": "为 true 时若目标不存在则 404 WORKSPACE_FILE_MISSING"
                    }
                ],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/octet-stream": {
                            "schema": { "type": "string", "format": "binary" }
                        }
                    }
                },
                "responses": {
                    "204": { "description": "已写入" },
                    "4XX": { "description": "路径、标志冲突或过大（JSON ApiError）" }
                }
            }
        },
        "/workspace/file/download": {
            "get": {
                "tags": ["workspace"],
                "summary": "读取工作区文件原始字节（任意类型；上限 16 MiB；供 Client 保存到本机）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "path",
                        "in": "query",
                        "required": true,
                        "schema": { "type": "string" },
                        "description": "工作区相对路径；禁止 `..`"
                    }
                ],
                "responses": {
                    "200": {
                        "description": "文件字节",
                        "content": {
                            "application/octet-stream": {
                                "schema": { "type": "string", "format": "binary" }
                            }
                        }
                    },
                    "4XX": { "description": "路径或大小错误（JSON ApiError）" }
                }
            }
        },
        "/workspace/dir/archive": {
            "get": {
                "tags": ["workspace"],
                "summary": "将工作区目录打包为 zip（未压缩合计与条目有上限；不跟随符号链接）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "path",
                        "in": "query",
                        "required": false,
                        "schema": { "type": "string" },
                        "description": "相对目录；省略或空表示工作区根；禁止 `..`"
                    }
                ],
                "responses": {
                    "200": {
                        "description": "zip 字节",
                        "content": {
                            "application/zip": {
                                "schema": { "type": "string", "format": "binary" }
                            }
                        }
                    },
                    "4XX": { "description": "路径、非目录或过大（JSON ApiError）" }
                }
            }
        },
        "/workspace/file/move": {
            "post": {
                "tags": ["workspace"],
                "summary": "移动或重命名工作区常规文件（非目录；可选写入会话变更集）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/WorkspaceFileMoveBody" }
                        }
                    }
                },
                "responses": {
                    "204": { "description": "已移动" },
                    "4XX": { "description": "路径、冲突或源不是文件（JSON ApiError）" }
                }
            }
        }
    })
}

pub(super) fn openapi_paths_fragment_skills() -> Value {
    json!({
        "/skills": {
            "get": {
                "tags": ["skills"],
                "summary": "列出当前工作区 skills（供 composer `/` 浮层）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "responses": {
                    "200": {
                        "description": "skills 目录 JSON",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/SkillsListResponse" }
                            }
                        }
                    }
                }
            }
        }
    })
}

pub(super) fn openapi_paths_fragment_workspace_file() -> Value {
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
                "summary": "删除工作区文件（path 单文件；或 paths 批量删除，任一非法则整批拒绝）",
                "security": [{ "bearerAuth": [] }, { "apiKeyAuth": [] }],
                "parameters": [
                    {
                        "name": "path",
                        "in": "query",
                        "required": false,
                        "description": "单路径删除的目标相对路径（批量删除时省略）",
                        "schema": { "type": "string" }
                    },
                    {
                        "name": "paths",
                        "in": "query",
                        "required": false,
                        "description": "批量删除：逗号分隔的相对路径列表（最多 32 个；任一非法则整批拒绝、不产生部分删除）",
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
        }
    })
}
