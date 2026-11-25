//! `memory` 组声明：`GET /memory/list`、`DELETE /memory/{id}`。
//!
//! 本组是声明表里**第一次出现 path 参数**（`/memory/{id}` 的 `id` → `"in": "path"`），
//! 也是第一次用到 `SchemaDecl::Integer`（`limit` / `offset` 带 min/max/default，`id` 只有
//! `format: int64`）与 `SchemaDecl::StrEnumDefault`（`sort`）。契约见
//! `docs/design/memory_management_api.md` §3。

use super::{
    HttpMethod, Override, ParamDecl, ParamLocation, RespBody, RespDecl, RouteDecl, SchemaDecl,
};

static MEMORY_DECLS: &[RouteDecl] = &[
    RouteDecl {
        path: "/memory/list",
        method: HttpMethod::Get,
        tag: "memory",
        summary: "按 scope 投影长期记忆列表（只读）",
        description: Some(
            "scope 即 `conversation_id`；响应不含 embedding 与内部 source_role/source_kind。空 scope 返回 200 空列表。长期记忆未启用时 503 LONG_TERM_MEMORY_DISABLED（响应体含 enabled:false）。",
        ),
        protected: true,
        params: &[
            ParamDecl {
                name: "conversation_id",
                location: ParamLocation::Query,
                required: true,
                description: Some("作用域；仅允许字母、数字、- _ . :，长度上限 128"),
                schema: SchemaDecl::Str,
            },
            ParamDecl {
                name: "limit",
                location: ParamLocation::Query,
                required: false,
                description: Some("每页条数；clamp 到 1..=200"),
                schema: SchemaDecl::Integer {
                    format: Some("int32"),
                    minimum: Some(1),
                    maximum: Some(200),
                    default: Some(50),
                },
            },
            ParamDecl {
                name: "offset",
                location: ParamLocation::Query,
                required: false,
                description: None,
                schema: SchemaDecl::Integer {
                    format: Some("int32"),
                    minimum: Some(0),
                    maximum: None,
                    default: Some(0),
                },
            },
            ParamDecl {
                name: "kind",
                location: ParamLocation::Query,
                required: false,
                description: None,
                schema: SchemaDecl::StrEnum(&["turn", "explicit", "experience", "other"]),
            },
            ParamDecl {
                name: "tag",
                location: ParamLocation::Query,
                required: false,
                description: Some("命中 tags_json 中的标签"),
                schema: SchemaDecl::Str,
            },
            ParamDecl {
                name: "q",
                location: ParamLocation::Query,
                required: false,
                description: Some("chunk_text 子串（非 FTS）"),
                schema: SchemaDecl::Str,
            },
            ParamDecl {
                name: "sort",
                location: ParamLocation::Query,
                required: false,
                description: None,
                schema: SchemaDecl::StrEnumDefault {
                    values: &["created_at_desc", "created_at_asc"],
                    default: "created_at_desc",
                },
            },
        ],
        request: None,
        responses: &[
            RespDecl {
                status: "200",
                description: "投影列表",
                body: RespBody::Json(SchemaDecl::Ref("MemoryListResponse")),
            },
            RespDecl {
                status: "400",
                description: "INVALID_CONVERSATION_ID / INVALID_MEMORY_KIND",
                body: RespBody::None,
            },
            RespDecl {
                status: "503",
                description: "LONG_TERM_MEMORY_DISABLED（enabled=false）",
                body: RespBody::None,
            },
        ],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/memory/{id}",
        method: HttpMethod::Delete,
        tag: "memory",
        summary: "按 id 删除长期记忆（幂等）",
        description: Some(
            "行不存在 / 已过期 / 不属于该 conversation_id 均返回 204（不泄露 id 是否存在）。",
        ),
        protected: true,
        params: &[
            ParamDecl {
                name: "id",
                location: ParamLocation::Path,
                required: true,
                description: None,
                schema: SchemaDecl::Integer {
                    format: Some("int64"),
                    minimum: None,
                    maximum: None,
                    default: None,
                },
            },
            ParamDecl {
                name: "conversation_id",
                location: ParamLocation::Query,
                required: true,
                description: Some("作用域守卫（必填）"),
                schema: SchemaDecl::Str,
            },
        ],
        request: None,
        responses: &[
            RespDecl {
                status: "204",
                description: "已删除（或本就不存在 / 不属于该 scope）",
                body: RespBody::None,
            },
            RespDecl {
                status: "400",
                description: "INVALID_CONVERSATION_ID / INVALID_MEMORY_ID",
                body: RespBody::None,
            },
            RespDecl {
                status: "503",
                description: "LONG_TERM_MEMORY_DISABLED（enabled=false）",
                body: RespBody::None,
            },
        ],
        override_kind: Override::None,
    },
];

pub(crate) fn decls() -> &'static [RouteDecl] {
    MEMORY_DECLS
}
