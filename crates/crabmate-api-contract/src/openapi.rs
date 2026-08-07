//! OpenAPI `components.schemas` 片段（由 `schemars` 从契约类型生成）。

use schemars::JsonSchema;
use serde_json::{Map, Value};

use crate::api::{
    ApiError, ConfigReloadResponseBody, DeleteUploadsBody, DeleteUploadsResponseBody,
    SessionConversationStoreRequestBody, UploadResponseBody, UploadedFileInfo,
};
use crate::chat::{
    ChatApprovalRequestBody, ChatApprovalResponseBody, ChatAsyncRequestBodyOpenApi,
    ChatAsyncSubmitResponseBody, ChatBranchRequestBody, ChatBranchResponseBody,
    ChatJobStatusResponseBody, ChatRequestBodyWire, ChatResponseBody, ClientLlmBody,
    ConversationMessagesResponseBodyOpenApi, ExecutorLlmBody, StreamResumeBody,
};
use crate::status::StatusShellView;
use crate::web_ui::WebUiConfigResponse;

fn schema_value<T: JsonSchema>() -> Value {
    let root = schemars::schema_for!(T);
    serde_json::to_value(root).expect("schema serializes")
}

/// 由契约类型生成的 OpenAPI schema 对象（键为 schema 名称）。
pub fn openapi_component_schemas() -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("ApiError".into(), schema_value::<ApiError>());
    map.insert("ClientLlmBody".into(), schema_value::<ClientLlmBody>());
    map.insert("ExecutorLlmBody".into(), schema_value::<ExecutorLlmBody>());
    map.insert(
        "ChatRequestBody".into(),
        schema_value::<ChatRequestBodyWire>(),
    );
    map.insert(
        "ChatAsyncRequestBody".into(),
        schema_value::<ChatAsyncRequestBodyOpenApi>(),
    );
    map.insert(
        "ChatResponseBody".into(),
        schema_value::<ChatResponseBody>(),
    );
    map.insert(
        "ChatAsyncSubmitResponseBody".into(),
        schema_value::<ChatAsyncSubmitResponseBody>(),
    );
    map.insert(
        "ChatJobStatusResponseBody".into(),
        schema_value::<ChatJobStatusResponseBody>(),
    );
    map.insert(
        "ChatApprovalRequestBody".into(),
        schema_value::<ChatApprovalRequestBody>(),
    );
    map.insert(
        "ChatApprovalResponseBody".into(),
        schema_value::<ChatApprovalResponseBody>(),
    );
    map.insert(
        "ChatBranchRequestBody".into(),
        schema_value::<ChatBranchRequestBody>(),
    );
    map.insert(
        "ChatBranchResponseBody".into(),
        schema_value::<ChatBranchResponseBody>(),
    );
    map.insert(
        "StreamResumeBody".into(),
        schema_value::<StreamResumeBody>(),
    );
    map.insert(
        "ConversationMessagesResponseBody".into(),
        schema_value::<ConversationMessagesResponseBodyOpenApi>(),
    );
    map.insert(
        "ConfigReloadResponseBody".into(),
        schema_value::<ConfigReloadResponseBody>(),
    );
    map.insert(
        "UploadedFileInfo".into(),
        schema_value::<UploadedFileInfo>(),
    );
    map.insert(
        "UploadResponseBody".into(),
        schema_value::<UploadResponseBody>(),
    );
    map.insert(
        "DeleteUploadsBody".into(),
        schema_value::<DeleteUploadsBody>(),
    );
    map.insert(
        "DeleteUploadsResponseBody".into(),
        schema_value::<DeleteUploadsResponseBody>(),
    );
    map.insert(
        "SessionConversationStoreRequestBody".into(),
        schema_value::<SessionConversationStoreRequestBody>(),
    );
    map.insert(
        "WebUiConfigResponse".into(),
        schema_value::<WebUiConfigResponse>(),
    );
    map.insert("StatusShellView".into(), schema_value::<StatusShellView>());
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
}
