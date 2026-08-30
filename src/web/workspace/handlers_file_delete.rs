//! 工作区文件删除：`DELETE /workspace/file`（单路径 `path` 或批量 `paths` 逗号分隔）。
//! 批量语义：先整体校验（任一非法 → 整批拒绝、不产生部分删除），再逐个删除并汇总成功/失败。

use axum::{
    Json,
    extract::{Query, State},
};

use super::handlers::effective_workspace_base_canonical;
#[cfg(unix)]
use super::handlers_sync::workspace_delete_file_sync_unix;
use crate::web::app_state::AppStateHttpCore;
use crate::web::http_types::validation::validate_workspace_query_encoding_optional;
use crate::web::http_types::workspace::{
    WorkspaceFileDeleteFailure, WorkspaceFileDeleteResponse, WorkspaceFileQuery,
};
use crate::workspace::path::resolve_web_workspace_read_path;

fn workspace_file_delete_err(msg: String) -> Json<WorkspaceFileDeleteResponse> {
    Json(WorkspaceFileDeleteResponse {
        error: Some(msg),
        ..Default::default()
    })
}

async fn workspace_file_delete_resolve(
    http: &AppStateHttpCore,
    query: &WorkspaceFileQuery,
) -> Result<(std::path::PathBuf, std::path::PathBuf), Json<WorkspaceFileDeleteResponse>> {
    let base_canonical = match effective_workspace_base_canonical(http).await {
        Ok(p) => p,
        Err(e) => return Err(workspace_file_delete_err(e.user_message())),
    };
    if let Err(e) = validate_workspace_query_encoding_optional(query.encoding.as_deref()) {
        return Err(workspace_file_delete_err(e));
    }
    if query.path.trim().is_empty() {
        return Err(workspace_file_delete_err("path 不能为空".to_string()));
    }
    match resolve_web_workspace_read_path(&base_canonical, Some(query.path.as_str())) {
        Ok(canonical) => Ok((base_canonical, canonical)),
        Err(e) => Err(workspace_file_delete_err(e.user_message())),
    }
}

#[cfg(unix)]
async fn workspace_file_delete_unix(
    base_canonical: std::path::PathBuf,
    canonical: std::path::PathBuf,
) -> Json<WorkspaceFileDeleteResponse> {
    match tokio::task::spawn_blocking(move || {
        workspace_delete_file_sync_unix(base_canonical, canonical)
    })
    .await
    {
        Ok(Ok(())) => Json(WorkspaceFileDeleteResponse::default()),
        Ok(Err(msg)) => workspace_file_delete_err(msg),
        Err(e) => workspace_file_delete_err(format!("删除文件任务失败: {}", e)),
    }
}

#[cfg(not(unix))]
async fn workspace_file_delete_non_unix(
    canonical: std::path::PathBuf,
) -> Json<WorkspaceFileDeleteResponse> {
    let meta = match tokio::fs::metadata(&canonical).await {
        Ok(m) => m,
        Err(e) => {
            return workspace_file_delete_err(format!("无法读取文件信息: {}", e));
        }
    };
    if meta.is_dir() {
        return workspace_file_delete_err("不支持删除目录".to_string());
    }
    match tokio::fs::remove_file(&canonical).await {
        Ok(()) => Json(WorkspaceFileDeleteResponse::default()),
        Err(e) => workspace_file_delete_err(format!("删除文件失败: {}", e)),
    }
}

/// 删除工作区内的文件：`path` 单文件删除；或 `paths` 批量删除（任一非法则整批拒绝、不产生部分删除）。不能删除目录。
pub async fn workspace_file_delete_handler(
    State(http): State<AppStateHttpCore>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Json<WorkspaceFileDeleteResponse> {
    let batch_paths: Vec<String> = query
        .paths
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if !batch_paths.is_empty() {
        return workspace_files_delete_batch(&http, &batch_paths).await;
    }

    let (base_canonical, canonical) = match workspace_file_delete_resolve(&http, &query).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    #[cfg(unix)]
    {
        workspace_file_delete_unix(base_canonical, canonical).await
    }
    #[cfg(not(unix))]
    {
        let _ = base_canonical;
        workspace_file_delete_non_unix(canonical).await
    }
}

/// 批量删除：先**整体校验**（任一非法 → 整批拒绝、不产生部分删除），再**逐个删除**并汇总成功/失败（继续删完）。
async fn workspace_files_delete_batch(
    http: &AppStateHttpCore,
    paths: &[String],
) -> Json<WorkspaceFileDeleteResponse> {
    if paths.is_empty() {
        return workspace_file_delete_err("paths 不能为空".to_string());
    }
    if paths.len() > 32 {
        return workspace_file_delete_err(format!(
            "一次最多删除 32 个文件（收到 {} 个）",
            paths.len()
        ));
    }

    // 校验阶段：全部路径须可解析、在工作区根内、且是文件。
    let base_canonical = match effective_workspace_base_canonical(http).await {
        Ok(p) => p,
        Err(e) => return workspace_file_delete_err(e.user_message()),
    };
    let mut resolved = Vec::with_capacity(paths.len());
    for raw in paths {
        let path = raw.trim();
        if path.is_empty() {
            return workspace_file_delete_err("paths 中含空路径".to_string());
        }
        match resolve_web_workspace_read_path(&base_canonical, Some(path)) {
            Ok(canonical) => {
                if canonical.is_dir() {
                    return workspace_file_delete_err(format!("不支持删除目录：{path}"));
                }
                resolved.push((path.to_string(), canonical));
            }
            Err(e) => return workspace_file_delete_err(e.user_message()),
        }
    }

    // 删除阶段：继续删完，逐文件汇总结果。
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    for (path, canonical) in resolved {
        match workspace_file_delete_one(&base_canonical, canonical).await {
            Ok(()) => deleted.push(path),
            Err(msg) => failed.push(WorkspaceFileDeleteFailure { path, error: msg }),
        }
    }
    Json(WorkspaceFileDeleteResponse {
        error: None,
        deleted,
        failed,
    })
}

#[cfg(unix)]
async fn workspace_file_delete_one(
    base_canonical: &std::path::Path,
    canonical: std::path::PathBuf,
) -> Result<(), String> {
    let base = base_canonical.to_path_buf();
    match tokio::task::spawn_blocking(move || {
        workspace_delete_file_sync_unix(base, canonical)
    })
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(msg)) => Err(msg),
        Err(e) => Err(format!("删除文件任务失败: {e}")),
    }
}

#[cfg(not(unix))]
async fn workspace_file_delete_one(
    _base_canonical: &std::path::Path,
    canonical: std::path::PathBuf,
) -> Result<(), String> {
    let meta = match tokio::fs::metadata(&canonical).await {
        Ok(m) => m,
        Err(e) => return Err(format!("无法读取文件信息: {e}")),
    };
    if meta.is_dir() {
        return Err("不支持删除目录".to_string());
    }
    tokio::fs::remove_file(&canonical)
        .await
        .map_err(|e| format!("删除文件失败: {e}"))
}
