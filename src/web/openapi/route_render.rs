//! 把 [`crate::web::routes::decl`] 的声明表渲染成 OpenAPI `paths` 片段（issue #1382）。
//!
//! 只负责**骨架**：`tags` / `security` / `parameters` / `responses` 的嵌套外壳、
//! `content` 的 `application/json` 包裹、`$ref` 前缀。文案与 schema 语义全部来自声明表，
//! 本模块不加工。
//!
//! 输出与改造前的手写 `json!` 逐字段相同：`serde_json` 的 `Map` 未启用 `preserve_order`，
//! object 键序由 `BTreeMap` 决定、与插入序无关；只有数组（`tags` / `parameters` /
//! `oneOf`）顺序要紧，声明表按原顺序书写。

use serde_json::{Map, Value, json};

use crate::web::routes::decl::{Override, ParamDecl, RespDecl, RouteDecl, SchemaDecl};

use super::openapi_helpers::{json_content, protected_security};

/// 渲染一组声明为 `paths` 片段（单个 object：path → 该 path 下的 method 表）。
pub(super) fn render_route_decls(decls: &[RouteDecl]) -> Value {
    let mut paths: Map<String, Value> = Map::new();
    for decl in decls {
        // `Conditional`：运行期条件注册、不进 OpenAPI 的端点，跳过不产出 path。
        if matches!(decl.override_kind, Override::Conditional) {
            continue;
        }
        let operation = render_operation(decl);
        let entry = paths
            .entry(decl.path.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(methods) = entry else {
            unreachable!("path entry is always created as an object");
        };
        let method = decl.method.as_str();
        assert!(
            methods.insert(method.to_string(), operation).is_none(),
            "duplicate OpenAPI path+method in route decl: {method} {}",
            decl.path
        );
    }
    Value::Object(paths)
}

fn render_operation(decl: &RouteDecl) -> Value {
    // 除 `Conditional`（调用方已跳过）外，其余 override 变体都还没接上消费逻辑。
    // 这里刻意用无条件 `assert!` 而非 `debug_assert!`：`build_openapi_spec()` 是运行时
    // 函数（`GET /openapi.json` 直接调它），release 下静默渲染出缺片段的契约比直接失败更危险。
    assert!(
        matches!(decl.override_kind, Override::None),
        "override_kind 尚未接入渲染路径，登记前先实现消费（{} {}）",
        decl.method.as_str(),
        decl.path
    );

    let mut operation = Map::new();
    operation.insert("tags".to_string(), json!([decl.tag]));
    operation.insert("summary".to_string(), json!(decl.summary));
    if let Some(description) = decl.description {
        operation.insert("description".to_string(), json!(description));
    }
    if decl.protected {
        operation.insert("security".to_string(), protected_security());
    }
    if !decl.params.is_empty() {
        operation.insert(
            "parameters".to_string(),
            Value::Array(decl.params.iter().map(render_param).collect()),
        );
    }
    operation.insert("responses".to_string(), render_responses(decl));
    Value::Object(operation)
}

/// 渲染 `parameters`：`"in"` 由 [`ParamDecl::location`] 决定，不再写死 query。
fn render_param(param: &ParamDecl) -> Value {
    let mut out = Map::new();
    out.insert("name".to_string(), json!(param.name));
    out.insert("in".to_string(), json!(param.location.as_str()));
    out.insert("required".to_string(), json!(param.required));
    out.insert("schema".to_string(), render_schema(param.schema));
    // 无描述就不写这个键：现存手写 OpenAPI 里有 9 个参数没有描述，
    // 补空串会变成 `"description": ""`，与原契约不同。
    if let Some(description) = param.description {
        out.insert("description".to_string(), json!(description));
    }
    Value::Object(out)
}

fn render_responses(decl: &RouteDecl) -> Value {
    let mut responses = Map::new();
    for resp in decl.responses {
        assert!(
            responses
                .insert(resp.status.to_string(), render_response_body(resp))
                .is_none(),
            "duplicate response status {} on {} {}",
            resp.status,
            decl.method.as_str(),
            decl.path
        );
    }
    Value::Object(responses)
}

fn render_response_body(resp: &RespDecl) -> Value {
    let mut body = Map::new();
    body.insert("description".to_string(), json!(resp.description));
    // `schema: None` 表示**有意**无 `content`（`4XX` / `5XX` 的纯状态码说明），不是漏写；
    // 漏写会表现为「该有 schema 的 200 变成了纯说明」，由 `docs/openapi.json` 快照测试拦住。
    if let Some(schema) = resp.schema {
        body.insert("content".to_string(), json_content(render_schema(schema)));
    }
    Value::Object(body)
}

fn render_schema(schema: SchemaDecl) -> Value {
    match schema {
        SchemaDecl::Ref(name) => schema_ref(name),
        SchemaDecl::Object => json!({ "type": "object" }),
        SchemaDecl::StrEnum([]) => json!({ "type": "string" }),
        SchemaDecl::StrEnum(values) => json!({ "type": "string", "enum": values }),
        SchemaDecl::OneOfRefs(names) => json!({
            "oneOf": names.iter().map(|name| schema_ref(name)).collect::<Vec<_>>()
        }),
    }
}

fn schema_ref(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::routes::decl::{HttpMethod, ParamLocation};

    static QUERY_PARAM: ParamDecl = ParamDecl {
        name: "view",
        location: ParamLocation::Query,
        required: false,
        description: Some("视图"),
        schema: SchemaDecl::StrEnum(&["shell"]),
    };

    static MOCK_DECLS: [RouteDecl; 2] = [
        RouteDecl {
            path: "/mock",
            method: HttpMethod::Get,
            tag: "system",
            summary: "读",
            description: Some("读说明"),
            protected: true,
            params: &[QUERY_PARAM],
            responses: &[RespDecl {
                status: "200",
                description: "读取结果",
                schema: Some(SchemaDecl::Ref("MockView")),
            }],
            override_kind: Override::None,
        },
        RouteDecl {
            path: "/mock",
            method: HttpMethod::Post,
            tag: "system",
            summary: "写",
            description: None,
            protected: false,
            params: &[],
            responses: &[
                RespDecl {
                    status: "200",
                    description: "写入结果",
                    schema: Some(SchemaDecl::Object),
                },
                RespDecl {
                    status: "4XX",
                    description: "参数错误",
                    schema: None,
                },
            ],
            override_kind: Override::None,
        },
    ];

    #[test]
    fn groups_methods_under_one_path_and_renders_skeleton() {
        let rendered = render_route_decls(&MOCK_DECLS);
        let get = &rendered["/mock"]["get"];
        assert_eq!(get["tags"], json!(["system"]));
        assert_eq!(get["summary"], json!("读"));
        assert_eq!(get["description"], json!("读说明"));
        assert_eq!(
            get["security"],
            json!([{ "bearerAuth": [] }, { "apiKeyAuth": [] }])
        );
        assert_eq!(
            get["parameters"],
            json!([{
                "name": "view",
                "in": "query",
                "required": false,
                "schema": { "type": "string", "enum": ["shell"] },
                "description": "视图"
            }])
        );
        assert_eq!(
            get["responses"]["200"]["content"],
            json!({ "application/json": { "schema": { "$ref": "#/components/schemas/MockView" } } })
        );

        let post = &rendered["/mock"]["post"];
        assert!(post.get("security").is_none(), "未受保护的端点不渲染 security");
        assert!(post.get("parameters").is_none(), "无参数时不渲染 parameters");
        assert_eq!(
            post["responses"]["200"]["content"],
            json!({ "application/json": { "schema": { "type": "object" } } })
        );
        assert!(
            post["responses"]["4XX"].get("content").is_none(),
            "schema 为 None 时不渲染 content"
        );
    }

    #[test]
    fn one_of_refs_preserve_declaration_order() {
        static DECLS: [RouteDecl; 1] = [RouteDecl {
            path: "/union",
            method: HttpMethod::Get,
            tag: "system",
            summary: "双视图",
            description: None,
            protected: false,
            params: &[],
            responses: &[RespDecl {
                status: "200",
                description: "合一",
                schema: Some(SchemaDecl::OneOfRefs(&["BView", "AView"])),
            }],
            override_kind: Override::None,
        }];
        assert_eq!(
            render_route_decls(&DECLS)["/union"]["get"]["responses"]["200"]["content"],
            json!({
                "application/json": {
                    "schema": {
                        "oneOf": [
                            { "$ref": "#/components/schemas/BView" },
                            { "$ref": "#/components/schemas/AView" }
                        ]
                    }
                }
            })
        );
    }

    #[test]
    fn empty_string_enum_renders_plain_string() {
        static DECLS: [RouteDecl; 1] = [RouteDecl {
            path: "/plain",
            method: HttpMethod::Get,
            tag: "system",
            summary: "无枚举",
            description: None,
            protected: false,
            params: &[ParamDecl {
                name: "q",
                location: ParamLocation::Query,
                required: true,
                description: Some("任意字符串"),
                schema: SchemaDecl::StrEnum(&[]),
            }],
            responses: &[],
            override_kind: Override::None,
        }];
        assert_eq!(
            render_route_decls(&DECLS)["/plain"]["get"]["parameters"][0]["schema"],
            json!({ "type": "string" }),
            "空 enum 列表退化为纯 string，不要渲染成空 enum 数组"
        );
    }

    #[test]
    fn renders_path_parameter_and_omits_absent_description() {
        static DECLS: [RouteDecl; 1] = [RouteDecl {
            path: "/memory/{id}",
            method: HttpMethod::Get,
            tag: "memory",
            summary: "按 id 读",
            description: None,
            protected: true,
            params: &[ParamDecl {
                name: "id",
                location: ParamLocation::Path,
                required: true,
                description: None,
                schema: SchemaDecl::StrEnum(&[]),
            }],
            responses: &[],
            override_kind: Override::None,
        }];
        let param = &render_route_decls(&DECLS)["/memory/{id}"]["get"]["parameters"][0];
        assert_eq!(param["in"], json!("path"), "in 必须由 location 决定，不能写死 query");
        assert_eq!(param["required"], json!(true));
        assert!(
            param.get("description").is_none(),
            "description 为 None 时不写该键（现存手写 OpenAPI 里有 9 个参数无描述）"
        );
    }

    #[test]
    fn conditional_override_is_not_rendered() {
        static DECLS: [RouteDecl; 2] = [
            RouteDecl {
                path: "/e2e/only",
                method: HttpMethod::Get,
                tag: "system",
                summary: "仅 e2e",
                description: None,
                protected: false,
                params: &[],
                responses: &[],
                override_kind: Override::Conditional,
            },
            RouteDecl {
                path: "/visible",
                method: HttpMethod::Get,
                tag: "system",
                summary: "常规",
                description: None,
                protected: false,
                params: &[],
                responses: &[],
                override_kind: Override::None,
            },
        ];
        let rendered = render_route_decls(&DECLS);
        assert!(
            rendered.get("/e2e/only").is_none(),
            "Conditional 不进 OpenAPI，必须跳过而不是照旧渲染"
        );
        assert!(rendered.get("/visible").is_some());
    }

    #[test]
    #[should_panic(expected = "duplicate OpenAPI path+method")]
    fn panics_on_duplicate_path_and_method() {
        static DUP: [RouteDecl; 2] = [
            RouteDecl {
                path: "/dup",
                method: HttpMethod::Get,
                tag: "system",
                summary: "a",
                description: None,
                protected: false,
                params: &[],
                responses: &[],
                override_kind: Override::None,
            },
            RouteDecl {
                path: "/dup",
                method: HttpMethod::Get,
                tag: "system",
                summary: "b",
                description: None,
                protected: false,
                params: &[],
                responses: &[],
                override_kind: Override::None,
            },
        ];
        let _ = render_route_decls(&DUP);
    }
}
