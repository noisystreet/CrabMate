//! Web 工作区「项目池」：列出 / 新建 / 切换命名子工作区。

use std::io;
use std::path::{Path, PathBuf};

use axum::{Json, extract::State, http::StatusCode};
use crabmate_web_host::http_types::workspace::{
    WorkspaceProjectPostBody, WorkspaceProjectPostResponse, WorkspaceProjectsListResponse,
};

use crate::web::app_state::AppStateHttpCore;
use crate::workspace::path::{
    WorkspacePathError, is_sensitive_workspace_path, validate_workspace_project_set_path,
    validate_workspace_set_path,
};
use crate::workspace::project::{
    WorkspaceProjectNameError, list_workspace_projects, workspace_project_dir,
};

use super::handlers::apply_workspace_override;

/// `GET /workspace/projects`：项目池是否启用及已有项目名列表。
pub async fn workspace_projects_list_handler(
    State(http): State<AppStateHttpCore>,
) -> Result<Json<WorkspaceProjectsListResponse>, (StatusCode, Json<serde_json::Value>)> {
    let cfg = http.cfg.read().await;
    let Some(pool) = cfg.workspace_roots.web_workspace_pool.clone() else {
        return Ok(Json(WorkspaceProjectsListResponse {
            enabled: false,
            pool_path: None,
            projects: vec![],
        }));
    };
    match list_workspace_projects(pool.as_path()) {
        Ok(projects) => Ok(Json(WorkspaceProjectsListResponse {
            enabled: true,
            pool_path: Some(pool.display().to_string()),
            projects,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "ok": false,
                "error": e,
            })),
        )),
    }
}

/// 在策略预检通过后确保项目目录存在（可选创建），再交由 [`validate_workspace_set_path`] 做最终校验。
pub(crate) fn ensure_workspace_project_dir(
    pool: &Path,
    raw_name: &str,
    create: bool,
) -> Result<PathBuf, EnsureProjectDirError> {
    let dir = workspace_project_dir(pool, raw_name).map_err(EnsureProjectDirError::Name)?;
    if is_sensitive_workspace_path(&dir) {
        return Err(EnsureProjectDirError::Sensitive);
    }
    if !dir.starts_with(pool) {
        return Err(EnsureProjectDirError::OutsidePool);
    }
    if dir.exists() {
        if dir.is_dir() {
            return Ok(dir);
        }
        return Err(EnsureProjectDirError::NotADirectory);
    }
    if !create {
        return Err(EnsureProjectDirError::NotFound);
    }
    match std::fs::create_dir(&dir) {
        Ok(()) => Ok(dir),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            if dir.is_dir() {
                Ok(dir)
            } else {
                Err(EnsureProjectDirError::NotADirectory)
            }
        }
        Err(e) => Err(EnsureProjectDirError::Create(e.to_string())),
    }
}

#[derive(Debug)]
pub(crate) enum EnsureProjectDirError {
    Name(WorkspaceProjectNameError),
    Sensitive,
    OutsidePool,
    NotADirectory,
    NotFound,
    Create(String),
}

impl EnsureProjectDirError {
    fn status_and_message(&self) -> (StatusCode, String) {
        match self {
            Self::Name(e) => (
                StatusCode::BAD_REQUEST,
                WorkspacePathError::InvalidProjectName(e.to_string()).user_message(),
            ),
            Self::Sensitive => (
                StatusCode::FORBIDDEN,
                WorkspacePathError::SensitivePathDenied.user_message(),
            ),
            Self::OutsidePool => (StatusCode::FORBIDDEN, "项目路径不在项目池内".to_string()),
            Self::NotADirectory => (StatusCode::BAD_REQUEST, "路径已存在且不是目录".to_string()),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "项目不存在；可勾选「新建」或传 create=true".to_string(),
            ),
            Self::Create(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("创建项目目录失败: {msg}"),
            ),
        }
    }
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
    let dir = ensure_workspace_project_dir(pool.as_path(), name, body.create).map_err(|e| {
        let (status, msg) = e.status_and_message();
        project_err(status, name, &msg)
    })?;
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
    use std::fs;

    #[test]
    fn list_empty_when_pool_missing_on_disk() {
        let missing = PathBuf::from("/nonexistent/pool/for_test_only");
        assert!(list_workspace_projects(&missing).unwrap().is_empty());
    }

    #[test]
    fn ensure_create_then_idempotent_already_exists() {
        let root = tempfile::tempdir().expect("tempdir");
        let pool = root.path();
        let dir = ensure_workspace_project_dir(pool, "app1", true).expect("create");
        assert!(dir.is_dir());
        let again = ensure_workspace_project_dir(pool, "app1", true).expect("idempotent");
        assert_eq!(dir, again);
        let open_only = ensure_workspace_project_dir(pool, "app1", false).expect("open");
        assert_eq!(dir, open_only);
    }

    #[test]
    fn ensure_not_found_without_create() {
        let root = tempfile::tempdir().expect("tempdir");
        let err =
            ensure_workspace_project_dir(root.path(), "missing", false).expect_err("should miss");
        assert!(matches!(err, EnsureProjectDirError::NotFound));
    }

    #[test]
    fn ensure_rejects_existing_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let blocker = root.path().join("blocker");
        fs::write(&blocker, b"x").expect("write");
        let err =
            ensure_workspace_project_dir(root.path(), "blocker", true).expect_err("file blocks");
        assert!(matches!(err, EnsureProjectDirError::NotADirectory));
    }

    #[test]
    fn ensure_rejects_invalid_name_before_create() {
        let root = tempfile::tempdir().expect("tempdir");
        let err = ensure_workspace_project_dir(root.path(), "../x", true).expect_err("bad name");
        assert!(matches!(err, EnsureProjectDirError::Name(_)));
        assert!(
            fs::read_dir(root.path())
                .expect("read_dir")
                .next()
                .is_none(),
            "invalid name must not create entries"
        );
    }
}
