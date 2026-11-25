//! `workspace` 组声明：14 个 `/workspace*` path、20 个 `(path, method)` 操作。
//!
//! 来源散在三处手写片段（`openapi_paths_workspace.rs`、`openapi_paths.rs` 的
//! `openapi_paths_fragment_workspace_list` / `_workspace_file_meta`）。文案逐字照抄，
//! 判据是 `docs/openapi.json` 快照零 diff。
//!
//! 本组第一次用到：`Str` / `Bool` 基础 schema、`RespBody::Media` 多 media type 响应
//! （图片、zip、octet-stream）、`MediaDecl::Binary` 请求体、`MediaDecl::JsonInline` 请求体。

use serde_json::json;

use super::{
    HttpMethod, MediaDecl, MediaTypeDecl, Override, ParamDecl, ParamLocation, RequestBody,
    RespBody, RespDecl, RouteDecl, SchemaDecl,
};

/// `path` query 参数：必填，原契约无描述（`GET /workspace/file`、`DELETE /workspace/dir`）。
const PATH_REQUIRED_NO_DESC: ParamDecl = ParamDecl {
    name: "path",
    location: ParamLocation::Query,
    required: true,
    description: None,
    schema: SchemaDecl::Str,
};

/// `path` query 参数：必填 + 「禁止 `..`」（raw PUT、download）。
const PATH_REQUIRED_STRICT: ParamDecl = ParamDecl {
    name: "path",
    location: ParamLocation::Query,
    required: true,
    description: Some("工作区相对路径；禁止 `..`"),
    schema: SchemaDecl::Str,
};

const OCTET_STREAM_BIN: MediaTypeDecl = MediaTypeDecl {
    media: "application/octet-stream",
    schema: SchemaDecl::Binary,
};

/// `/workspace/clone/stream` 的请求体：尚未具名化为 component，故内联。
fn clone_stream_body() -> serde_json::Value {
    json!({
        "type": "object",
        "required": ["url", "name"],
        "properties": {
            "url": { "type": "string" },
            "name": { "type": "string" },
            "depth": { "type": "integer", "minimum": 1 },
            "branch": { "type": "string" }
        }
    })
}

static WORKSPACE_DECLS: &[RouteDecl] = &[
    RouteDecl {
        path: "/workspace",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "列出工作区目录项",
        description: None,
        protected: true,
        params: &[ParamDecl {
            name: "path",
            location: ParamLocation::Query,
            required: false,
            description: Some("相对子路径，可选"),
            schema: SchemaDecl::Str,
        }],
        request: None,
        responses: &[RespDecl {
            status: "200",
            description: "目录列表",
            body: RespBody::Json(SchemaDecl::Ref("WorkspaceResponse")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace",
        method: HttpMethod::Post,
        tag: "workspace",
        summary: "设置当前 Web 工作区根",
        description: None,
        protected: true,
        params: &[],
        request: Some(RequestBody {
            required: false,
            media: MediaDecl::JsonRef("WorkspaceSetBody"),
        }),
        responses: &[RespDecl {
            status: "200",
            description: "设置结果（JSON，形状与实现一致）",
            body: RespBody::None,
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/pick",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "工作区选目录占位（始终 path=null；请在侧栏手动填写路径）",
        description: None,
        protected: true,
        params: &[],
        request: None,
        responses: &[RespDecl {
            status: "200",
            description: "所选路径或 null",
            body: RespBody::Json(SchemaDecl::Ref("WorkspacePickResponse")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/projects",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "列出 Web 项目池状态与项目名",
        description: None,
        protected: true,
        params: &[],
        request: None,
        responses: &[RespDecl {
            status: "200",
            description: "enabled / pool_path / projects",
            body: RespBody::None,
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/projects",
        method: HttpMethod::Post,
        tag: "workspace",
        summary: "按名称打开或新建项目池工作区",
        description: None,
        protected: true,
        params: &[],
        request: None,
        responses: &[RespDecl {
            status: "200",
            description: "切换结果（含 path）",
            body: RespBody::None,
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/clone/stream",
        method: HttpMethod::Post,
        tag: "workspace",
        summary: "在项目池内 git clone（SSE 进度；成功后切换当前工作区）",
        description: Some("仅当已配置 web_workspace_pool。请求 JSON：url、name，可选 depth、branch。成功路径：phase → log/progress → phase=activate → done(name,path)。墙钟超时 20 分钟。"),
        protected: true,
        params: &[],
        request: Some(RequestBody {
            required: true,
            media: MediaDecl::JsonInline(clone_stream_body),
        }),
        responses: &[
            RespDecl {
                status: "200",
                description: "text/event-stream（phase/log/progress/done/error）",
                body: RespBody::None,
            },
            RespDecl {
                status: "400",
                description: "CLONE_BAD_URL / CLONE_BAD_NAME / CLONE_BAD_DEPTH 等",
                body: RespBody::None,
            },
            RespDecl {
                status: "404",
                description: "CLONE_NO_POOL",
                body: RespBody::None,
            },
            RespDecl {
                status: "409",
                description: "CLONE_DIR_EXISTS",
                body: RespBody::None,
            },
            RespDecl {
                status: "429",
                description: "CLONE_BUSY（并发上限）",
                body: RespBody::None,
            },
            RespDecl {
                status: "503",
                description: "CLONE_NO_GIT",
                body: RespBody::None,
            },
        ],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/search",
        method: HttpMethod::Post,
        tag: "workspace",
        summary: "工作区内搜索",
        description: None,
        protected: true,
        params: &[],
        request: Some(RequestBody {
            required: true,
            media: MediaDecl::JsonRef("WorkspaceSearchBody"),
        }),
        responses: &[RespDecl {
            status: "200",
            description: "搜索结果文本",
            body: RespBody::Json(SchemaDecl::Ref("WorkspaceSearchResponse")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/dir",
        method: HttpMethod::Post,
        tag: "workspace",
        summary: "在工作区内创建目录，或通过 JSON delete=true 删除目录（与 DELETE 等价）",
        description: None,
        protected: true,
        params: &[],
        request: Some(RequestBody {
            required: true,
            media: MediaDecl::JsonRef("WorkspaceDirCreateBody"),
        }),
        responses: &[RespDecl {
            status: "200",
            description: "创建结果",
            body: RespBody::Json(SchemaDecl::Ref("WorkspaceDirCreateResponse")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/dir",
        method: HttpMethod::Delete,
        tag: "workspace",
        summary: "删除工作区目录（须 confirm=true；非空目录须 recursive=true）",
        description: None,
        protected: true,
        params: &[
            PATH_REQUIRED_NO_DESC,
            ParamDecl {
                name: "confirm",
                location: ParamLocation::Query,
                required: true,
                description: None,
                schema: SchemaDecl::Bool,
            },
            ParamDecl {
                name: "recursive",
                location: ParamLocation::Query,
                required: false,
                description: None,
                schema: SchemaDecl::Bool,
            },
        ],
        request: None,
        responses: &[RespDecl {
            status: "200",
            description: "删除结果",
            body: RespBody::Json(SchemaDecl::Ref("WorkspaceDirDeleteResponse")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/dir/archive",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "将工作区目录打包为 zip（未压缩合计与条目有上限；不跟随符号链接）",
        description: None,
        protected: true,
        params: &[ParamDecl {
            name: "path",
            location: ParamLocation::Query,
            required: false,
            description: Some("相对目录；省略或空表示工作区根；禁止 `..`"),
            schema: SchemaDecl::Str,
        }],
        request: None,
        responses: &[
            RespDecl {
                status: "200",
                description: "zip 字节",
                body: RespBody::Media(&[MediaTypeDecl {
                    media: "application/zip",
                    schema: SchemaDecl::Binary,
                }]),
            },
            RespDecl {
                status: "4XX",
                description: "路径、非目录或过大（JSON ApiError）",
                body: RespBody::None,
            },
        ],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/file",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "读取工作区内文本文件（有大小上限）",
        description: None,
        protected: true,
        params: &[
            PATH_REQUIRED_NO_DESC,
            ParamDecl {
                name: "encoding",
                location: ParamLocation::Query,
                required: false,
                description: Some("如 utf-8、gb18030、auto 等，与 `read_file` 工具一致"),
                schema: SchemaDecl::Str,
            },
        ],
        request: None,
        responses: &[
            RespDecl {
                status: "200",
                description: "文件正文或 JSON 包装（与实现一致）",
                body: RespBody::None,
            },
            RespDecl {
                status: "4XX",
                description: "路径或编码错误",
                body: RespBody::None,
            },
        ],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/file",
        method: HttpMethod::Post,
        tag: "workspace",
        summary: "写入工作区文件",
        description: None,
        protected: true,
        params: &[],
        request: Some(RequestBody {
            required: true,
            media: MediaDecl::JsonRef("WorkspaceFileWriteBody"),
        }),
        responses: &[RespDecl {
            status: "200",
            description: "写入结果",
            body: RespBody::Json(SchemaDecl::Ref("WorkspaceFileWriteResponse")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/file",
        method: HttpMethod::Delete,
        tag: "workspace",
        summary: "删除工作区文件（path 单文件；或 paths 批量删除，任一非法则整批拒绝）",
        description: None,
        protected: true,
        params: &[
            ParamDecl {
                name: "path",
                location: ParamLocation::Query,
                required: false,
                description: Some("单路径删除的目标相对路径（批量删除时省略）"),
                schema: SchemaDecl::Str,
            },
            ParamDecl {
                name: "paths",
                location: ParamLocation::Query,
                required: false,
                description: Some("批量删除：逗号分隔的相对路径列表（最多 32 个；任一非法则整批拒绝、不产生部分删除）"),
                schema: SchemaDecl::Str,
            },
        ],
        request: None,
        responses: &[RespDecl {
            status: "200",
            description: "删除结果",
            body: RespBody::Json(SchemaDecl::Ref("WorkspaceFileDeleteResponse")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/file/raw",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "读取工作区内常见图片原始字节（png/jpg/jpeg/webp/gif；上限 8 MiB）",
        description: None,
        protected: true,
        params: &[ParamDecl {
            name: "path",
            location: ParamLocation::Query,
            required: true,
            description: Some("工作区相对路径；禁止 `..`；不含 svg"),
            schema: SchemaDecl::Str,
        }],
        request: None,
        responses: &[
            RespDecl {
                status: "200",
                description: "图片字节",
                body: RespBody::Media(&[
                    MediaTypeDecl {
                        media: "image/png",
                        schema: SchemaDecl::Binary,
                    },
                    MediaTypeDecl {
                        media: "image/jpeg",
                        schema: SchemaDecl::Binary,
                    },
                    MediaTypeDecl {
                        media: "image/webp",
                        schema: SchemaDecl::Binary,
                    },
                    MediaTypeDecl {
                        media: "image/gif",
                        schema: SchemaDecl::Binary,
                    },
                ]),
            },
            RespDecl {
                status: "4XX",
                description: "路径、类型或大小错误（JSON ApiError）",
                body: RespBody::None,
            },
        ],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/file/raw",
        method: HttpMethod::Put,
        tag: "workspace",
        summary: "写入工作区文件原始字节（任意类型；上限 16 MiB；与 POST /workspace/file 的 JSON content 上限一致）",
        description: None,
        protected: true,
        params: &[
            PATH_REQUIRED_STRICT,
            ParamDecl {
                name: "create_only",
                location: ParamLocation::Query,
                required: false,
                description: Some("为 true 时若目标已存在则 409 WORKSPACE_FILE_EXISTS"),
                schema: SchemaDecl::Bool,
            },
            ParamDecl {
                name: "update_only",
                location: ParamLocation::Query,
                required: false,
                description: Some("为 true 时若目标不存在则 404 WORKSPACE_FILE_MISSING"),
                schema: SchemaDecl::Bool,
            },
        ],
        request: Some(RequestBody {
            required: true,
            media: MediaDecl::Binary("application/octet-stream"),
        }),
        responses: &[
            RespDecl {
                status: "204",
                description: "已写入",
                body: RespBody::None,
            },
            RespDecl {
                status: "4XX",
                description: "路径、标志冲突或过大（JSON ApiError）",
                body: RespBody::None,
            },
        ],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/file/download",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "读取工作区文件原始字节（任意类型；上限 16 MiB；供 Client 保存到本机）",
        description: None,
        protected: true,
        params: &[PATH_REQUIRED_STRICT],
        request: None,
        responses: &[
            RespDecl {
                status: "200",
                description: "文件字节",
                body: RespBody::Media(&[OCTET_STREAM_BIN]),
            },
            RespDecl {
                status: "4XX",
                description: "路径或大小错误（JSON ApiError）",
                body: RespBody::None,
            },
        ],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/file/move",
        method: HttpMethod::Post,
        tag: "workspace",
        summary: "移动或重命名工作区常规文件（非目录；可选写入会话变更集）",
        description: None,
        protected: true,
        params: &[],
        request: Some(RequestBody {
            required: true,
            media: MediaDecl::JsonRef("WorkspaceFileMoveBody"),
        }),
        responses: &[
            RespDecl {
                status: "204",
                description: "已移动",
                body: RespBody::None,
            },
            RespDecl {
                status: "4XX",
                description: "路径、冲突或源不是文件（JSON ApiError）",
                body: RespBody::None,
            },
        ],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/git",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "当前工作区 git 状态只读投影（分支 + 脏摘要 + 可选文件清单）",
        description: Some(
            "一律 200，用 `state` 表达可用性：`ok` / `not_a_repo` / `git_unavailable` / `failed`（后者看 `reason_code`）。\
             `dirty` 只代表「当前工作区根下」有未提交改动，**不**代表仓库干净。\
             `source` 为 `probe` / `cache` / `stale_cache`；`refresh=true` 只跳过缓存读，仍回写缓存。\
             `files=true` 才带 `files`（`items` / `truncated` / `limit`），与 `counts` 同一次探测同口径；\
             `path` 为工作区根相对、不带引号或转义，未跟踪目录折叠成一条 `kind=dir`。",
        ),
        protected: true,
        params: &[
            ParamDecl {
                name: "refresh",
                location: ParamLocation::Query,
                required: false,
                description: Some("跳过缓存读，强制重新探测（仍回写缓存）"),
                schema: SchemaDecl::Bool,
            },
            ParamDecl {
                name: "files",
                location: ParamLocation::Query,
                required: false,
                description: Some("返回未提交改动的文件清单（默认不返回，避免高频轮询搬整份列表）"),
                schema: SchemaDecl::Bool,
            },
        ],
        request: None,
        responses: &[RespDecl {
            status: "200",
            description: "git 状态投影（四态）",
            body: RespBody::Json(SchemaDecl::Ref("WorkspaceGitStatusResponse")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/profile",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "项目画像 Markdown",
        description: None,
        protected: true,
        params: &[],
        request: None,
        responses: &[RespDecl {
            status: "200",
            description: "画像",
            body: RespBody::Json(SchemaDecl::Ref("WorkspaceProfileResponse")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/workspace/changelog",
        method: HttpMethod::Get,
        tag: "workspace",
        summary: "本会话工作区变更集 Markdown",
        description: None,
        protected: true,
        params: &[ParamDecl {
            name: "conversation_id",
            location: ParamLocation::Query,
            required: false,
            description: None,
            schema: SchemaDecl::Str,
        }],
        request: None,
        responses: &[RespDecl {
            status: "200",
            description: "变更集",
            body: RespBody::Json(SchemaDecl::Ref("WorkspaceChangelogResponse")),
        }],
        override_kind: Override::None,
    },
];

pub(crate) fn decls() -> &'static [RouteDecl] {
    WORKSPACE_DECLS
}
