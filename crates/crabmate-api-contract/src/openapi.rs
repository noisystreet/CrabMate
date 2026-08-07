//! OpenAPI `components.schemas` 片段（由 `schemars` 从契约类型生成）。

use schemars::JsonSchema;
use serde_json::{Map, Value};

use crate::api::{ApiError, ConfigReloadResponseBody, UploadResponseBody};
use crate::chat::{
    ChatApprovalRequestBody, ChatApprovalResponseBody, ChatAsyncSubmitResponseBody,
    ChatBranchRequestBody, ChatBranchResponseBody, ChatJobStatusResponseBody, ChatResponseBody,
    ClientLlmBody, ExecutorLlmBody, StreamResumeBody,
};
use crate::status::StatusShellView;

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
    map.insert("StreamResumeBody".into(), schema_value::<StreamResumeBody>());
    map.insert(
        "ConfigReloadResponseBody".into(),
        schema_value::<ConfigReloadResponseBody>(),
    );
    map.insert(
        "UploadResponseBody".into(),
        schema_value::<UploadResponseBody>(),
    );
    map.insert("StatusShellView".into(), schema_value::<StatusShellView>());
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_schemas_include_status_shell_view() {
        let schemas = openapi_component_schemas();
        assert!(schemas.contains_key("StatusShellView"));
        assert!(schemas.contains_key("ApiError"));
        assert!(schemas.contains_key("ClientLlmBody"));
    }
}
