//! OpenAPI `components.schemas`：从 `schemars` 生成并适配为 **OpenAPI 3.0** 形状。
//!
//! schemars 默认 JSON Schema 含 `$schema` / `$defs` / `type: [T,"null"]` / `items: true` 等，
//! 不能直接塞进本仓库声明的 OAS 3.0.3 `components.schemas`。

use schemars::JsonSchema;
use serde_json::{Map, Value, json};

use crate::cm_api_contract::api::{
    ApiError, ConfigReloadResponseBody, DeleteUploadsBody, DeleteUploadsResponseBody,
    SessionConversationStoreRequestBody, UploadResponseBody, UploadedFileInfo,
};
use crate::cm_api_contract::chat::{
    ChatApprovalRequestBody, ChatApprovalResponseBody, ChatAsyncRequestBodyOpenApi,
    ChatAsyncSubmitResponseBody, ChatBranchRequestBody, ChatBranchResponseBody,
    ChatJobStatusResponseBody, ChatRequestBodyWire, ChatResponseBody, ClientLlmBody,
    ConversationMessagesResponseBodyOpenApi, ExecutorLlmBody, StreamResumeBody,
};
use crate::cm_api_contract::status::StatusShellView;
use crate::cm_api_contract::web_ui::WebUiConfigResponse;

fn schema_value_raw<T: JsonSchema>() -> Value {
    let root = schemars::schema_for!(T);
    serde_json::to_value(root).expect("schema serializes")
}

/// 剥掉根上的 `$defs`，返回 (本体 schema, defs map)。
fn peel_defs(mut root: Value) -> (Value, Map<String, Value>) {
    let mut defs = Map::new();
    if let Value::Object(ref mut obj) = root {
        obj.remove("$schema");
        if let Some(Value::Object(d)) = obj.remove("$defs") {
            defs = d;
        }
        // 部分生成器用 `definitions`
        if let Some(Value::Object(d)) = obj.remove("definitions") {
            for (k, v) in d {
                defs.entry(k).or_insert(v);
            }
        }
    }
    (root, defs)
}

fn adapt_type_null_array(map: &mut Map<String, Value>) {
    let Some(Value::Array(types)) = map.get("type").cloned() else {
        return;
    };
    let mut non_null = Vec::new();
    let mut has_null = false;
    for t in types {
        if t.as_str() == Some("null") {
            has_null = true;
        } else {
            non_null.push(t);
        }
    }
    if !has_null {
        return;
    }
    match non_null.len() {
        0 => {
            map.insert("type".into(), Value::String("object".into()));
        }
        1 => {
            map.insert("type".into(), non_null.pop().expect("len 1"));
        }
        _ => {
            map.insert("type".into(), Value::Array(non_null));
        }
    }
    map.insert("nullable".into(), Value::Bool(true));
}

fn adapt_bare_value_property(map: &mut Map<String, Value>) {
    if map.contains_key("type")
        || map.contains_key("$ref")
        || map.contains_key("anyOf")
        || map.contains_key("oneOf")
        || map.contains_key("allOf")
        || map.contains_key("properties")
        || map.contains_key("items")
    {
        return;
    }
    if !(map.contains_key("description") || map.contains_key("default")) {
        return;
    }
    map.insert("type".into(), Value::String("object".into()));
    map.insert("additionalProperties".into(), Value::Bool(true));
}

fn adapt_items_true(map: &mut Map<String, Value>) {
    if matches!(map.get("items"), Some(Value::Bool(true))) {
        map.insert(
            "items".into(),
            json!({
                "type": "object",
                "additionalProperties": true
            }),
        );
    }
}

/// `type: [T, "null"]` → `type: T` + `nullable: true`；`items: true` → 开放 object。
fn adapt_node(node: &mut Value) {
    match node {
        Value::Object(map) => {
            adapt_items_true(map);
            adapt_type_null_array(map);
            adapt_bare_value_property(map);
            for (k, v) in map.iter_mut() {
                if k == "$ref" {
                    if let Some(s) = v.as_str() {
                        *v = Value::String(rewrite_ref(s));
                    }
                } else {
                    adapt_node(v);
                }
            }
        }
        Value::Array(arr) => {
            for v in arr {
                adapt_node(v);
            }
        }
        _ => {}
    }
}

fn rewrite_ref(r: &str) -> String {
    if let Some(name) = r.strip_prefix("#/$defs/") {
        return format!("#/components/schemas/{name}");
    }
    if let Some(name) = r.strip_prefix("#/definitions/") {
        return format!("#/components/schemas/{name}");
    }
    r.to_string()
}

fn insert_adapted_schema<T: JsonSchema>(out: &mut Map<String, Value>, name: &str) {
    let raw = schema_value_raw::<T>();
    let (mut body, nested_defs) = peel_defs(raw);
    adapt_node(&mut body);
    for (def_name, def_raw) in nested_defs {
        let (mut peeled, inner) = peel_defs(def_raw);
        for (ik, mut iv) in inner {
            adapt_node(&mut iv);
            out.entry(ik).or_insert(iv);
        }
        adapt_node(&mut peeled);
        out.entry(def_name).or_insert(peeled);
    }
    if out.insert(name.to_string(), body).is_some() {
        panic!("duplicate OpenAPI schema key while building contract schemas: {name}");
    }
}

/// 由契约类型生成的 OpenAPI 3.0 schema 对象（键为 schema 名称）。
pub fn openapi_component_schemas() -> Map<String, Value> {
    let mut map = Map::new();
    insert_adapted_schema::<ApiError>(&mut map, "ApiError");
    insert_adapted_schema::<ClientLlmBody>(&mut map, "ClientLlmBody");
    insert_adapted_schema::<ExecutorLlmBody>(&mut map, "ExecutorLlmBody");
    insert_adapted_schema::<StreamResumeBody>(&mut map, "StreamResumeBody");
    insert_adapted_schema::<ChatRequestBodyWire>(&mut map, "ChatRequestBody");
    insert_adapted_schema::<ChatAsyncRequestBodyOpenApi>(&mut map, "ChatAsyncRequestBody");
    insert_adapted_schema::<ChatResponseBody>(&mut map, "ChatResponseBody");
    insert_adapted_schema::<ChatAsyncSubmitResponseBody>(&mut map, "ChatAsyncSubmitResponseBody");
    insert_adapted_schema::<ChatJobStatusResponseBody>(&mut map, "ChatJobStatusResponseBody");
    insert_adapted_schema::<ChatApprovalRequestBody>(&mut map, "ChatApprovalRequestBody");
    insert_adapted_schema::<ChatApprovalResponseBody>(&mut map, "ChatApprovalResponseBody");
    insert_adapted_schema::<ChatBranchRequestBody>(&mut map, "ChatBranchRequestBody");
    insert_adapted_schema::<ChatBranchResponseBody>(&mut map, "ChatBranchResponseBody");
    insert_adapted_schema::<ConversationMessagesResponseBodyOpenApi>(
        &mut map,
        "ConversationMessagesResponseBody",
    );
    insert_adapted_schema::<ConfigReloadResponseBody>(&mut map, "ConfigReloadResponseBody");
    insert_adapted_schema::<UploadedFileInfo>(&mut map, "UploadedFileInfo");
    insert_adapted_schema::<UploadResponseBody>(&mut map, "UploadResponseBody");
    insert_adapted_schema::<DeleteUploadsBody>(&mut map, "DeleteUploadsBody");
    insert_adapted_schema::<DeleteUploadsResponseBody>(&mut map, "DeleteUploadsResponseBody");
    insert_adapted_schema::<SessionConversationStoreRequestBody>(
        &mut map,
        "SessionConversationStoreRequestBody",
    );
    insert_adapted_schema::<WebUiConfigResponse>(&mut map, "WebUiConfigResponse");
    insert_adapted_schema::<StatusShellView>(&mut map, "StatusShellView");
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_schemas_include_core_chat_and_status() {
        let schemas = openapi_component_schemas();
        for key in [
            "StatusShellView",
            "ApiError",
            "ClientLlmBody",
            "ChatRequestBody",
            "ChatAsyncRequestBody",
            "ConversationMessagesResponseBody",
            "WebUiConfigResponse",
            "DeleteUploadsBody",
        ] {
            assert!(schemas.contains_key(key), "missing schema {key}");
        }
    }

    #[test]
    fn chat_request_body_schema_requires_message() {
        let schemas = openapi_component_schemas();
        let chat = schemas
            .get("ChatRequestBody")
            .expect("ChatRequestBody schema");
        let required = chat
            .pointer("/required")
            .and_then(|v| v.as_array())
            .expect("required array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("message")),
            "ChatRequestBody must require message"
        );
    }

    #[test]
    fn openapi_schemas_are_oas30_shaped() {
        let schemas = openapi_component_schemas();
        for (name, schema) in &schemas {
            assert!(
                schema.get("$schema").is_none(),
                "{name} must not contain $schema"
            );
            assert!(
                schema.get("$defs").is_none(),
                "{name} must not contain $defs (hoist to components)"
            );
            assert_no_json_schema_null_type(name, schema);
            assert_no_items_true(name, schema);
            assert_refs_point_to_components(name, schema);
        }

        let chat = &schemas["ChatRequestBody"];
        let session_mode = chat
            .pointer("/properties/session_mode")
            .expect("session_mode");
        let enum_vals = session_mode
            .get("enum")
            .and_then(|v| v.as_array())
            .expect("session_mode enum");
        assert!(enum_vals.iter().any(|v| v.as_str() == Some("ask")));
        assert!(enum_vals.iter().any(|v| v.as_str() == Some("plan")));
        assert!(enum_vals.iter().any(|v| v.as_str() == Some("act")));

        let answers = chat
            .pointer("/properties/clarify_questionnaire_answers/properties/answers")
            .or_else(|| {
                // answers 可能在提升后的 ClarifyQuestionnaireAnswersBody 内
                schemas
                    .get("ClarifyQuestionnaireAnswersBody")
                    .and_then(|s| s.pointer("/properties/answers"))
            })
            .expect("answers schema");
        assert_eq!(answers.get("type").and_then(|t| t.as_str()), Some("object"));

        let messages = schemas["ConversationMessagesResponseBody"]
            .pointer("/properties/messages/items")
            .expect("messages.items");
        assert!(
            messages.is_object(),
            "messages.items must be a schema object"
        );
        assert_ne!(messages, &Value::Bool(true));
    }

    fn assert_no_json_schema_null_type(path: &str, node: &Value) {
        match node {
            Value::Object(m) => {
                if let Some(Value::Array(types)) = m.get("type") {
                    assert!(
                        !types.iter().any(|t| t.as_str() == Some("null")),
                        "{path}: use nullable:true instead of type array with null"
                    );
                }
                for (k, v) in m {
                    assert_no_json_schema_null_type(&format!("{path}.{k}"), v);
                }
            }
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    assert_no_json_schema_null_type(&format!("{path}[{i}]"), v);
                }
            }
            _ => {}
        }
    }

    fn assert_no_items_true(path: &str, node: &Value) {
        match node {
            Value::Object(m) => {
                assert!(
                    !matches!(m.get("items"), Some(Value::Bool(true))),
                    "{path}: items:true is not valid OAS 3.0"
                );
                for (k, v) in m {
                    assert_no_items_true(&format!("{path}.{k}"), v);
                }
            }
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    assert_no_items_true(&format!("{path}[{i}]"), v);
                }
            }
            _ => {}
        }
    }

    fn assert_refs_point_to_components(path: &str, node: &Value) {
        match node {
            Value::Object(m) => {
                if let Some(Value::String(r)) = m.get("$ref") {
                    assert!(
                        r.starts_with("#/components/schemas/") || !r.starts_with("#/"),
                        "{path}: $ref {r} must use #/components/schemas/"
                    );
                    assert!(!r.contains("$defs"), "{path}: $ref must not point at $defs");
                }
                for (k, v) in m {
                    assert_refs_point_to_components(&format!("{path}.{k}"), v);
                }
            }
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    assert_refs_point_to_components(&format!("{path}[{i}]"), v);
                }
            }
            _ => {}
        }
    }
}
