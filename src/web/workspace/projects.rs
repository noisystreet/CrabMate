//! Web 工作区「项目池」：列出 / 新建 / 切换命名子工作区。

use axum::{Json, extract::State, http::StatusCode};
use crabmate_web_host::http_types::workspace::{
    WorkspaceProjectPostBody, WorkspaceProjectPostResponse, WorkspaceProjectsListResponse,
};

use crate::web::app_state::AppStateHttpCore;
use crate::workspace::path::{
    WorkspacePathError, validate_workspace_project_set_path, validate_workspace_set_path,
};
use crate::workspace::project::{list_workspace_projects, workspace_project_dir};

use super::handlers::apply_workspace_override;

/// `GET /workspace/projects`：项目池是否启用及已有项目名列表。
pub async fn workspace_projects_list_handler(
    State(http): State<AppStateHttpCore>,
) -> Json<WorkspaceProjectsListResponse> {
    let cfg = http.cfg.read().await;
    let Some(pool) = cfg.workspace_roots.web_workspace_pool.clone() else {
        return Json(WorkspaceProjectsListResponse {
            enabled: false,
            pool_path: None,
            projects: vec![],
        });
    };
    let projects = list_workspace_projects(pool.as_path()).unwrap_or_default();
    Json(WorkspaceProjectsListResponse {
        enabled: true,
        pool_path: Some(pool.display().to_string()),
        projects,
    })
}

/// `POST /workspace/projects`：按名称创建（可选）并切换当前 Web 工作区。
pub async fn workspace_projects_post_handler(
    State(http): State<AppStateHttpCore>,
    Json(body): Json<WorkspaceProjectPostBody>,
) -> Result<Json<WorkspaceProjectPostResponse>, (StatusCode, Json<WorkspaceProjectPostResponse>)> {
    let cfg = http.cfg.read().await;
    let pool = cfg
        .workspace_roots
        .web_workspace_pool
        .clone()
        .ok_or_else(|| project_err(StatusCode::NOT_FOUND, "", "未配置 web_workspace_pool"))?;
    let name = body.name.trim();
    let dir = workspace_project_dir(pool.as_path(), name).map_err(|e| {
        project_err(
            StatusCode::BAD_REQUEST,
            name,
            &WorkspacePathError::InvalidProjectName(e.to_string()).user_message(),
        )
    })?;
    if !dir.exists() {
        if body.create {
            if let Some(parent) = dir.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    project_err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        name,
                        &format!("创建项目目录失败: {e}"),
                    )
                })?;
            }
            std::fs::create_dir(&dir).map_err(|e| {
                project_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    name,
                    &format!("创建项目目录失败: {e}"),
                )
            })?;
        } else {
            return Err(project_err(
                StatusCode::NOT_FOUND,
                name,
                "项目不存在；可勾选「新建」或传 create=true",
            ));
        }
    } else if !dir.is_dir() {
        return Err(project_err(
            StatusCode::BAD_REQUEST,
            name,
            "路径已存在且不是目录",
        ));
    }
    let canon = match validate_workspace_set_path(&cfg, &dir.display().to_string()) {
        Ok(p) => p,
        Err(e) => {
            let status = if e.is_policy_denied() {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::BAD_REQUEST
            };
            return Err(project_err(status, name, &e.user_message()));
        }
    };
    let path_str = canon.display().to_string();
    let validated_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    drop(cfg);
    apply_workspace_override(&http, &path_str).await;
    Ok(Json(WorkspaceProjectPostResponse {
        ok: true,
        name: validated_name,
        path: path_str,
        error: None,
    }))
}

/// 解析 `POST /workspace` 的 `project` 字段为绝对路径（目录须已存在）。
pub fn resolve_workspace_set_project_path(
    cfg: &crabmate_config::AgentConfig,
    project: &str,
) -> Result<std::path::PathBuf, WorkspacePathError> {
    validate_workspace_project_set_path(cfg, project)
}

fn project_err(
    status: StatusCode,
    name: &str,
    msg: &str,
) -> (StatusCode, Json<WorkspaceProjectPostResponse>) {
    (
        status,
        Json(WorkspaceProjectPostResponse {
            ok: false,
            name: name.to_string(),
            path: String::new(),
            error: Some(msg.to_string()),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn list_empty_when_pool_missing_on_disk() {
        let missing = PathBuf::from("/nonexistent/pool/for_test_only");
        assert!(list_workspace_projects(&missing).unwrap().is_empty());
    }
}
