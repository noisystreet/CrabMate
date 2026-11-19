//! `system` 组声明：`GET /openapi.json`、`/health`、`/status`、`/web-ui`。
//!
//! 四条 `.route(` 散在三处——`web/server.rs`（`/openapi.json`）、`routes/system/mod.rs`
//! （`/health`、`/status`）、`cm_web_host/routes/web_ui.rs`（`/web-ui`，经 `system::router()`
//! merge）。表按语义分组，故能收在一处；`cm_web_host` 的注册与再导出结构不动。
//!
//! 文案与 `openapi_paths.rs` 原手写片段逐字一致 —— `docs/openapi.json` 快照测试是判据。

use super::{HttpMethod, Override, ParamDecl, ParamLocation, RespDecl, RouteDecl, SchemaDecl};

/// `GET /status?view=shell`：切到壳层稳定子集。
const VIEW_PARAM: ParamDecl = ParamDecl {
    name: "view",
    location: ParamLocation::Query,
    required: false,
    description: Some("为 shell 时返回 Web 壳层稳定子集 StatusShellView；省略则返回完整运行状态（StatusResponseBody）"),
    schema: SchemaDecl::StrEnum(&["shell"]),
};

static SYSTEM_DECLS: [RouteDecl; 4] = [
    RouteDecl {
        path: "/openapi.json",
        method: HttpMethod::Get,
        tag: "system",
        summary: "OpenAPI 本文档",
        description: None,
        protected: false,
        params: &[],
        responses: &[RespDecl {
            status: "200",
            description: "OpenAPI 3.0 JSON",
            schema: Some(SchemaDecl::Object),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/health",
        method: HttpMethod::Get,
        tag: "system",
        summary: "健康检查（依赖、可选 LLM models 探活等）",
        description: None,
        protected: false,
        params: &[],
        responses: &[RespDecl {
            status: "200",
            description: "健康报告 JSON（status + checks；checks 键随运行环境动态增减）",
            schema: Some(SchemaDecl::Ref("HealthReportView")),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/status",
        method: HttpMethod::Get,
        tag: "system",
        summary: "运行状态（模型、工具数、会话模式默认、规划配置等）",
        description: None,
        protected: false,
        params: &[VIEW_PARAM],
        responses: &[RespDecl {
            status: "200",
            description: "状态 JSON",
            schema: Some(SchemaDecl::OneOfRefs(&[
                "StatusShellView",
                "StatusResponseBody",
            ])),
        }],
        override_kind: Override::None,
    },
    RouteDecl {
        path: "/web-ui",
        method: HttpMethod::Get,
        tag: "system",
        summary: "CSR 展示开关（Markdown、助手展示过滤；受 CM_WEB_* 环境变量影响）",
        description: None,
        protected: false,
        params: &[],
        responses: &[RespDecl {
            status: "200",
            description: "Web UI 配置 JSON",
            schema: Some(SchemaDecl::Ref("WebUiConfigResponse")),
        }],
        override_kind: Override::None,
    },
];

pub(crate) fn decls() -> &'static [RouteDecl] {
    &SYSTEM_DECLS
}
