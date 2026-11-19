//! `GET /openapi.json`：OpenAPI 3.0 机器可读契约（与 `server.rs` 路由对齐；**不**替代 `docs/SSE协议.md` 对 SSE 行级语义的说明）。

mod fragment_merge;
mod openapi_components;
mod openapi_components_user_data;
mod openapi_helpers;
mod openapi_paths;
mod openapi_paths_chat_stream;
mod openapi_paths_permission;
mod openapi_paths_session_state;
mod openapi_paths_tool_jobs;
mod openapi_paths_tools;
mod openapi_paths_user_data;
mod openapi_paths_user_data_mcp;
mod openapi_paths_skills;
mod route_render;
#[cfg(test)]
mod route_table;

use serde_json::{Value, json};

use openapi_components::openapi_components_value;
use openapi_paths::openapi_paths_value;

/// 构建与当前 `serve` 路由表一致的 OpenAPI 文档（不含静态 `/` SPA）。
pub fn build_openapi_spec() -> Value {
    let version = env!("CARGO_PKG_VERSION");
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "CrabMate Web API",
            "version": version,
            "description": concat!(
                "CrabMate `serve` 模式的 HTTP 契约摘要。\n\n",
                "- **鉴权**：嵌入默认 **`web_api_require_bearer=false`**：允许无共享密钥启动 **`serve`**；若将 **`web_api_require_bearer=true`**，则启动前须配置非空 **`CM_WEB_API_BEARER_TOKEN`**（或 TOML **`web_api_bearer_token`**）。进程启动且密钥**非空**时，下列需鉴权路径须在请求头携带 **`Authorization: Bearer <token>`** 或 **`X-API-Key: <token>`**（与配置值为**同一密钥**，二选一）。密钥为空时中间件不校验，路径可对能访问监听地址的客户端匿名访问（仅限可信环境）。\n",
                "- **SSE**：`POST /chat/stream` 返回 `text/event-stream`；控制面 JSON 与错误码见仓库 `docs/SSE协议.md`，本 OpenAPI 仅作入口说明。\n",
                "- **上传**：`POST /upload` 使用 `multipart/form-data`。"
            )
        },
        "tags": [
            { "name": "chat", "description": "对话与流式 SSE" },
            { "name": "workspace", "description": "工作区浏览与文件" },
            { "name": "system", "description": "健康检查与状态" },
            { "name": "tasks", "description": "进程内任务清单" },
            { "name": "skills", "description": "工作区技能（元工具 skill_manage）" },
            { "name": "tools", "description": "工具清单、详情与进程级启停" },
            { "name": "tool_jobs", "description": "后台工具任务轮询与取消" },
            { "name": "config", "description": "配置热重载" },
            { "name": "memory", "description": "长期记忆只读列表与按 id 删除" },
            { "name": "user_data", "description": "本机用户数据（~/.local/share/crabmate）" },
            { "name": "uploads", "description": "上传与删除" }
        ],
        "paths": openapi_paths_value(),
        "components": openapi_components_value(),
    })
}

/// Axum handler：`application/json` OpenAPI 文档。
pub(crate) async fn openapi_json_handler() -> axum::Json<Value> {
    axum::Json(build_openapi_spec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn openapi_ops_match_axum_route_source() {
        let spec = build_openapi_spec();
        let documented = super::route_table::openapi_path_ops(&spec);
        let mounted =
            super::route_table::axum_route_ops_from_source(Path::new(env!("CARGO_MANIFEST_DIR")));
        let missing: Vec<_> = mounted.difference(&documented).cloned().collect();
        let extra: Vec<_> = documented.difference(&mounted).cloned().collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "OpenAPI path+method must match axum `.route(` in src/web/routes, src/web/server.rs, cm_web_host web_ui (not e2e/static)\nmissing from OpenAPI: {missing:?}\nextra in OpenAPI: {extra:?}"
        );
        assert!(
            documented.iter().all(|(path, _)| !path.starts_with("/e2e/")),
            "E2E fixture routes must not appear in OpenAPI"
        );
    }

    /// 声明表里的每个 `(path, method)` 都必须真的挂在 axum `.route(` 上。
    ///
    /// 表与注册之间只有字符串关联（Rust 无编译期绑定），这条是「表不能凭空多出端点」的护栏；
    /// 反方向（注册了但没进表）由 [`openapi_ops_match_axum_route_source`] 兜住，
    /// 分批迁移完成后两者合起来即为双向一致。
    #[test]
    fn declared_ops_are_mounted_in_axum_routes() {
        let mounted = super::route_table::axum_route_ops_from_source(Path::new(env!(
            "CARGO_MANIFEST_DIR"
        )));
        let missing: Vec<_> = crate::web::routes::decl::all()
            .into_iter()
            .map(|decl| (decl.method.as_str(), decl.path))
            .filter(|(method, path)| !mounted.contains(&(path.to_string(), method.to_string())))
            .collect();
        assert!(
            missing.is_empty(),
            "声明表里的端点没在源码 `.route(` 找到（method, path）：{missing:?}"
        );
    }

    /// 声明表登记的 component 名必须在 `components.schemas` 里存在。
    ///
    /// 表里写的是字符串而不是 `JsonSchema` 类型，拼错只会表现为「OpenAPI 里有个悬空 `$ref`」，
    /// 所以这里在渲染之外单独断言一次。
    #[test]
    fn declared_schema_refs_exist_in_components() {
        let spec = build_openapi_spec();
        let schemas = spec["components"]["schemas"]
            .as_object()
            .expect("components.schemas object");
        let mut missing = Vec::new();
        for decl in crate::web::routes::decl::all() {
            // `Conditional` 不进 OpenAPI（渲染器跳过该行），其引用的 schema 不必存在。
            if matches!(
                decl.override_kind,
                crate::web::routes::decl::Override::Conditional
            ) {
                continue;
            }
            let mut refs = Vec::new();
            decl.collect_refs(&mut refs);
            for name in refs {
                if !schemas.contains_key(name) {
                    missing.push((decl.path, name));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "声明表引用了不存在的 components schema（path, schema）：{missing:?}"
        );
    }

    /// `docs/openapi.json` 快照必须与 [`build_openapi_spec`] 一致（路由/契约变更时同步）。
    ///
    /// 再生成：`CRABMATE_BLESS=1 cargo test --lib web::openapi::tests::openapi_docs_snapshot_matches_spec`
    /// （版本号 bump 发布时同样需要；与 `man/crabmate.1` 同一维护模式。）
    /// 快照可导入 Swagger Editor / VS Code OpenAPI 扩展 / Postman 查阅或生成 client。
    #[test]
    fn openapi_docs_snapshot_matches_spec() {
        let spec = build_openapi_spec();
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/openapi.json");
        if std::env::var_os("CRABMATE_BLESS").is_some() {
            let pretty = serde_json::to_string_pretty(&spec).expect("serialize OpenAPI spec");
            std::fs::write(&path, pretty + "\n").expect("write docs/openapi.json");
            eprintln!("Wrote {}", path.display());
            return;
        }
        let raw = std::fs::read_to_string(&path).expect(
            "docs/openapi.json missing; regenerate with `CRABMATE_BLESS=1 cargo test --lib web::openapi::tests::openapi_docs_snapshot_matches_spec`",
        );
        let on_disk: Value = serde_json::from_str(&raw).expect("docs/openapi.json is valid JSON");
        if on_disk != spec {
            let drift: Vec<&str> = ["openapi", "info", "tags", "paths", "components"]
                .into_iter()
                .filter(|key| on_disk.get(*key) != spec.get(*key))
                .collect();
            panic!(
                "docs/openapi.json drifted from build_openapi_spec() (fields: {drift:?}); regenerate with `CRABMATE_BLESS=1 cargo test --lib web::openapi::tests::openapi_docs_snapshot_matches_spec`"
            );
        }
    }

    #[test]
    fn openapi_spec_has_core_paths_and_version() {
        let v = build_openapi_spec();
        assert_eq!(v["openapi"], "3.0.3");
        let paths = v["paths"].as_object().expect("paths object");
        assert!(paths.contains_key("/health"));
        assert!(paths.contains_key("/web-ui"));
        assert!(paths.contains_key("/chat/stream"));
        assert!(paths.contains_key("/chat/stream/{job_id}/cancel"));
        assert!(paths.contains_key("/chat/async"));
        assert!(paths.contains_key("/chat/jobs/{job_id}"));
        assert!(paths.contains_key("/tools/jobs/{tool_job_id}"));
        assert!(paths.contains_key("/tools/jobs/{tool_job_id}/output"));
        assert!(paths.contains_key("/tools/jobs/{tool_job_id}/cancel"));
        assert!(paths.contains_key("/conversation/messages"));
        assert!(paths.contains_key("/openapi.json"));
        assert!(paths.contains_key("/user-data/prefs"));
        assert!(paths.contains_key("/user-data/workspaces/current/sessions"));
        assert!(v["components"]["securitySchemes"]["bearerAuth"].is_object());
        assert!(v["components"]["securitySchemes"]["apiKeyAuth"].is_object());
        let schemas = v["components"]["schemas"]
            .as_object()
            .expect("schemas object");
        assert!(
            schemas.contains_key("ChatRequestBody"),
            "ChatRequestBody from contract"
        );
        assert!(
            schemas.contains_key("WebUiConfigResponse"),
            "WebUiConfigResponse from contract"
        );
    }
}
