//! OpenAPI 片段里与内容无关的共用样板（`#1240`）。
//!
//! 9 个 `openapi_paths_*` 分片里，`security` 数组逐字重复了 71 次、`content` 外壳
//! 重复了 80 余次；任何一处措辞或键名调整都要改十几遍。这里只抽**骨架**——
//! 具体 path、schema、description 仍留在各分片，保持「读一个文件就知道该组端点长什么样」。
//!
//! **契约零变化**：`build_openapi_spec()` 的返回值必须与抽样板前逐字段相同
//! （`docs/openapi.json` 快照测试会拦住任何漂移）。

use serde_json::{Value, json};

/// 受保护路由的 `security`：Bearer token 或 API-Key 二选一。
///
/// 与 `build_openapi_spec()` 的 `info.description` 里「同一密钥、二选一」的说法一致。
pub(super) fn protected_security() -> Value {
    json!([{ "bearerAuth": [] }, { "apiKeyAuth": [] }])
}

/// 响应 / 请求体的 `content` 外壳：`{ "application/json": { "schema": <schema> } }`。
pub(super) fn json_content(schema: Value) -> Value {
    json!({ "application/json": { "schema": schema } })
}
