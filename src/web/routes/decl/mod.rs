//! HTTP 端点的**声明表**：`(path, method)` 一行，OpenAPI `paths` 由它渲染（issue #1382）。
//!
//! 为什么放 `routes/` 侧而不是 `openapi/`：端点的存在是**路由**的事实，文档是它的投影。
//! 表按**语义分组**聚合，不按文件位置——例如 `system` 组的三条 `.route(` 分散在
//! `web/server.rs`、`routes/system/mod.rs`、`cm_web_host/routes/web_ui.rs`。
//!
//! 表与 `.route(` 之间只有字符串关联（Rust 没有编译期绑定），因此一致性由
//! [`crate::web::openapi`] 的对照测试兜住：声明表 `⊆` 源码 `.route(`，且
//! 每个登记的 component 名确实存在于 `components.schemas`。
//!
//! 表字段写不出的情形（流式响应、动态键、宽松对象、条件注册）必须用 [`Override`]
//! **显式登记**，不允许「漏了就悄悄少一段」。渲染器对「登记了但尚未实现消费」的变体直接
//! `assert!` 失败（debug 与 release 一致）；登记的 component 名是否存在由
//! `web::openapi` 的对照测试断言——表里写的是字符串，拼错不会成为编译错误。
//!
//! 分批迁移：本文件只定义形状与 `system` 组；其余分组按 `routes/` 逐批迁入（每批 1 commit）。

mod memory;
mod system;
mod workspace;

pub(crate) use memory::decls as memory_decls;
pub(crate) use system::decls as system_decls;
pub(crate) use workspace::decls as workspace_decls;

/// HTTP 方法。渲染时小写，与 axum `.route(.., get(..))` / OpenAPI 键一致。
///
/// 变体随分批迁移逐步启用：`system` 组只用到 [`HttpMethod::Get`]，其余方法随
/// `workspace` / `user_data` 等分组迁入。
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Post => "post",
            Self::Put => "put",
            Self::Patch => "patch",
            Self::Delete => "delete",
        }
    }
}

/// 表字段表达不了、必须保留手写片段的**显式登记**。
///
/// 变体随分批迁移逐步启用：`system` 组四条都能用结构化字段表达，故本批只用到
/// [`Override::None`]。先定形状是刻意的——让 override 成为「登记项」而不是
/// 「发现写不出来时补的一段 `json!`」。
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Override {
    /// 无需覆盖：`responses` 完全由表字段渲染。
    None,
    /// 非 JSON 响应。**当前无必需场景**：`text/event-stream` 的 schema 已能用
    /// `RespBody::Media` 表达，`/workspace/clone/stream` 用 `RespBody::None` 即可。
    /// 登记前请先确认表字段真的表达不了——登记后会撞上渲染器的 `assert!`。
    Streaming,
    /// 响应对象键随运行环境增减。
    DynamicKeys,
    /// `additionalProperties: true` 的开放对象。
    LooseObject,
    /// 运行期条件注册、**不进 OpenAPI** 的端点：渲染器跳过该行，不产出 path。
    /// 它仍在源码 `.route(` 里，因此「表 ⊆ 源码路由」的对照测试照样校验它。
    Conditional,
}

/// 引用 `components.schemas` 的 schema 表达式。
///
/// 只覆盖「能结构化表达」的几类；表达不了的在调用点用 [`Override`] 登记后保留原样 JSON。
#[derive(Clone, Copy, Debug)]
pub(crate) enum SchemaDecl {
    /// 指向 `components.schemas` 里已有的具名 schema。
    Ref(&'static str),
    /// `{ "type": "object" }`：键不固定但确实是个 object 的响应（如 OpenAPI 文档本体）。
    Object,
    /// `{ "type": "string" }`：无枚举约束的字符串（路径、编码名等）。
    Str,
    /// `{ "type": "string", "enum": [...] }`；空列表退化为 `{ "type": "string" }`。
    StrEnum(&'static [&'static str]),
    /// `{ "type": "string", "enum": [...], "default": ... }`（带默认值的枚举参数）。
    StrEnumDefault {
        values: &'static [&'static str],
        default: &'static str,
    },
    /// `{ "type": "integer", ... }`：只带 OpenAPI 里实际出现的约束。
    Integer {
        format: Option<&'static str>,
        minimum: Option<i64>,
        maximum: Option<i64>,
        default: Option<i64>,
    },
    /// 多个具名 schema 的 `oneOf`（同一端点按参数返回不同视图，如 `GET /status?view=shell`）。
    OneOfRefs(&'static [&'static str]),
    /// `{ "type": "boolean" }`：开关型 query 参数（`confirm` / `recursive` / `create_only` …）。
    Bool,
    /// `{ "type": "string", "format": "binary" }`：二进制上传 / 下载（图片、zip、octet-stream）。
    Binary,
}

impl SchemaDecl {
    /// 收集本表达式引用到的 component 名，供「名字必须存在」的测试断言使用。
    #[cfg(test)]
    pub(crate) fn collect_refs(&self, out: &mut Vec<&'static str>) {
        match self {
            Self::Ref(name) => out.push(name),
            Self::OneOfRefs(names) => out.extend_from_slice(names),
            Self::Object | Self::Str | Self::StrEnum(_) | Self::Bool | Self::Binary => {}
            Self::StrEnumDefault { .. } | Self::Integer { .. } => {}
        }
    }
}

/// 参数位置。
///
/// `header` 暂无端点使用，真有需求再加——不为「可能用得上」预留空转的变体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParamLocation {
    Query,
    /// 路径参数（如 `/memory/{id}` 的 `id`）。
    Path,
}

impl ParamLocation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Path => "path",
        }
    }
}

/// 一个参数。
///
/// `description` 是 `Option`：现存手写 OpenAPI 的 36 个 query 参数里有 9 个没有描述，
/// 用空串顶替会渲染成 `"description": ""`（与原输出不同，快照测试立刻红）。
///
/// `location` 必须显式写：渲染器按它出 `"in"`。若像早先那样在渲染器里写死 `query`，
/// 加了 path 参数也不会报错，只会静默把 `in` 写错——而已有端点的快照能挡，新增端点
/// 走 `CRABMATE_BLESS` 重写时挡不住。
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParamDecl {
    pub name: &'static str,
    pub location: ParamLocation,
    pub required: bool,
    pub description: Option<&'static str>,
    pub schema: SchemaDecl,
}

/// 一个响应。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RespDecl {
    /// `"200"` / `"400"` / `"4XX"` …
    pub status: &'static str,
    pub description: &'static str,
    pub body: RespBody,
}

/// 响应体：决定 `content` 段怎么渲染。
///
/// 二进制下载与图片端点有多个 media type，`application/json` 的单 schema 表达不了，
/// 所以这里用枚举而不是 `Option<SchemaDecl>`——加 `Media` 时编译器会逼着每处声明
/// 明确选一种，而不是默默退化成 JSON。
#[derive(Clone, Copy, Debug)]
pub(crate) enum RespBody {
    /// 无 `content`：纯状态码 + 说明（`4XX` / `5XX`，以及「形状与实现一致」的 200）。
    None,
    /// `application/json` + 该 schema。
    Json(SchemaDecl),
    /// 多个 media type（图片 / zip / octet-stream 下载）。
    Media(&'static [MediaTypeDecl]),
}

/// 一个 media type 及其 schema。
#[derive(Clone, Copy, Debug)]
pub(crate) struct MediaTypeDecl {
    /// `application/zip` / `image/png` / `application/octet-stream` …
    pub media: &'static str,
    pub schema: SchemaDecl,
}

/// 请求体（OpenAPI `requestBody`）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RequestBody {
    /// `requestBody.required`。注意 `POST /workspace` 原契约就没写（默认 false）。
    pub required: bool,
    pub media: MediaDecl,
}

/// 请求体的 media 描述。
#[derive(Clone, Copy, Debug)]
pub(crate) enum MediaDecl {
    /// `application/json` + `$ref` 到 `components.schemas`。
    JsonRef(&'static str),
    /// `application/json` + 原样 schema。用于尚未具名化的请求体
    /// （如 `/workspace/clone/stream` 的 `{url,name,depth?,branch?}`）。
    JsonInline(fn() -> serde_json::Value),
    /// 单一非 JSON media type + binary schema（`application/octet-stream`）。
    Binary(&'static str),
}

/// 一个 `(path, method)` 端点。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RouteDecl {
    pub path: &'static str,
    pub method: HttpMethod,
    /// 对应 OpenAPI `tags[0]`；与 `build_openapi_spec()` 里 `tags` 表的名字一致。
    pub tag: &'static str,
    pub summary: &'static str,
    pub description: Option<&'static str>,
    /// 是否渲染共享的 Bearer / API-Key `security`（受保护 API）。
    pub protected: bool,
    pub params: &'static [ParamDecl],
    pub request: Option<RequestBody>,
    pub responses: &'static [RespDecl],
    pub override_kind: Override,
}

impl RouteDecl {
    /// 收集该端点引用到的全部 component 名。
    #[cfg(test)]
    pub(crate) fn collect_refs(&self, out: &mut Vec<&'static str>) {
        for param in self.params {
            param.schema.collect_refs(out);
        }
        if let Some(MediaDecl::JsonRef(name)) = self.request.map(|body| body.media) {
            out.push(name);
        }
        for resp in self.responses {
            match resp.body {
                RespBody::Json(schema) => schema.collect_refs(out),
                RespBody::Media(items) => {
                    for item in items {
                        item.schema.collect_refs(out);
                    }
                }
                RespBody::None => {}
            }
        }
    }
}

/// 全部已迁入的分组。分批迁移期间按组拼接；迁完即可作为「端点真源」。
///
/// 只给对照测试用（声明表 `⊆` 源码 `.route(`、引用的 component 名都存在）；
/// 生产路径按组渲染，不走这里。
#[cfg(test)]
pub(crate) fn all() -> Vec<&'static RouteDecl> {
    let mut out: Vec<&'static RouteDecl> = Vec::new();
    out.extend(system_decls());
    out.extend(workspace_decls());
    out.extend(memory_decls());
    out
}
