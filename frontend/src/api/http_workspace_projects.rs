//! `/workspace/projects` 与项目池相关 API。

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crate::i18n::Locale;

use super::browser::{auth_headers, window};
use super::http::{fetch_json, fetch_json_with_body};

#[derive(Serialize)]
struct WorkspaceSetBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceProjectsListResponse {
    pub enabled: bool,
    #[serde(default)]
    pub pool_path: Option<String>,
    #[serde(default)]
    pub projects: Vec<String>,
}

#[derive(Serialize)]
struct WorkspaceProjectPostBody {
    name: String,
    create: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceProjectPostResponse {
    pub ok: bool,
    /// 服务端校验后的项目名（成功路径当前仅用 `path` 更新 UI）。
    #[allow(dead_code)]
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// `GET /workspace/projects`：项目池是否启用及已有项目列表。
pub async fn fetch_workspace_projects(
    loc: Locale,
) -> Result<WorkspaceProjectsListResponse, String> {
    fetch_json("GET", "/workspace/projects", None, loc).await
}

/// `POST /workspace/projects`：创建（可选）并切换到命名项目工作区。
pub async fn post_workspace_project(
    name: &str,
    create: bool,
    loc: Locale,
) -> Result<WorkspaceProjectPostResponse, String> {
    let body = serde_json::to_string(&WorkspaceProjectPostBody {
        name: name.to_string(),
        create,
    })
    .map_err(|e| e.to_string())?;
    fetch_json_with_body("POST", "/workspace/projects", &body, loc).await
}

/// `POST /workspace`（支持 `project` 字段）。
pub async fn post_workspace_set_inner(
    path: Option<String>,
    project: Option<String>,
    loc: Locale,
) -> Result<String, String> {
    let body =
        serde_json::to_string(&WorkspaceSetBody { path, project }).map_err(|e| e.to_string())?;
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    init.set_headers(&h);
    init.set_body(&JsValue::from_str(&body));
    let req = Request::new_with_str_and_init("/workspace", &init)
        .map_err(|e| format!("request: {:?}", e))?;
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
        .await
        .map_err(|e| format!("read body: {:?}", e))?;
    let s = text
        .as_string()
        .ok_or_else(|| crate::i18n::api_err_body_type(loc).to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&s).map_err(|_| crate::i18n::api_err_request_failed(loc))?;
    if resp.ok() {
        if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
            return Err(v
                .get("error")
                .and_then(|x| x.as_str())
                .unwrap_or(crate::i18n::api_err_workspace_set_failed(loc))
                .to_string());
        }
        return Ok(v
            .get("path")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string());
    }
    Err(v
        .get("error")
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string)
        .unwrap_or_else(|| format!("HTTP {}", resp.status())))
}
