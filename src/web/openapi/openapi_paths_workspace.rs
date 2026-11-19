//! `GET /skills` OpenAPI 片段。
//!
//! 同模块原有的 `/workspace*` 片段已迁入声明表 `web/routes/decl/workspace.rs`（issue #1382），
//! 只剩 `/skills` 仍是手写。

use serde_json::{Value, json};

pub(super) fn openapi_paths_fragment_skills() -> Value {
    json!({
        "/skills": {
            "get": {
                "tags": ["skills"],
                "summary": "列出当前工作区 skills（供 composer `/` 浮层）",
                "security": super::openapi_helpers::protected_security(),
                "responses": {
                    "200": {
                        "description": "skills 目录 JSON",
                        "content": super::openapi_helpers::json_content(json!({ "$ref": "#/components/schemas/SkillsListResponse" }))
                    }
                }
            }
        }
    })
}
