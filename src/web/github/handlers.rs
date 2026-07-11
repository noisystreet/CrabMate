//! GitHub 在线模式 HTTP handler（只读：仓库上下文与当前分支 PR checks）。

use std::path::Path;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;

use crate::AppState;
use crate::tools::web_api::{github_pr_current_checks, github_repo_context};
use crate::web::http_types::github::{
    GithubPrCurrentChecksData, GithubPrCurrentChecksResponse, GithubRepoContextData,
    GithubRepoContextResponse,
};
use crate::workspace::path::validate_effective_workspace_base;

async fn github_workspace_dir(state: &Arc<AppState>) -> Result<std::path::PathBuf, String> {
    let base_str = state.effective_workspace_path().await;
    if base_str.trim().is_empty() {
        return Err("工作区未设置".to_string());
    }
    let base = Path::new(&base_str);
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("无法解析工作区路径：{e}"))?;
    let cfg = state.http.cfg.read().await;
    validate_effective_workspace_base(&cfg, &base_canonical).map_err(|e| e.user_message())?;
    Ok(base_canonical)
}

struct GithubGhContext {
    allowed_commands: Vec<String>,
    max_output_len: usize,
    work_dir: std::path::PathBuf,
}

async fn github_gh_context(state: &Arc<AppState>) -> Result<GithubGhContext, String> {
    let work_dir = github_workspace_dir(state).await?;
    let cfg = state.http.cfg.read().await;
    Ok(GithubGhContext {
        allowed_commands: cfg.command_exec.allowed_commands.to_vec(),
        max_output_len: cfg.command_exec.command_max_output_len,
        work_dir,
    })
}

pub async fn github_repo_context_handler(
    State(state): State<Arc<AppState>>,
) -> Json<GithubRepoContextResponse> {
    let ctx = match github_gh_context(&state).await {
        Ok(c) => c,
        Err(e) => {
            return Json(GithubRepoContextResponse {
                data: GithubRepoContextData::default(),
                error: Some(e),
            });
        }
    };
    let GithubGhContext {
        allowed_commands,
        max_output_len,
        work_dir,
    } = ctx;
    match tokio::task::spawn_blocking(move || {
        github_repo_context(max_output_len, &allowed_commands, &work_dir)
    })
    .await
    {
        Ok(Ok(data)) => Json(GithubRepoContextResponse { data, error: None }),
        Ok(Err(e)) => Json(GithubRepoContextResponse {
            data: GithubRepoContextData::default(),
            error: Some(e),
        }),
        Err(e) => Json(GithubRepoContextResponse {
            data: GithubRepoContextData::default(),
            error: Some(format!("GitHub 上下文查询失败：{e}")),
        }),
    }
}

pub async fn github_pr_current_checks_handler(
    State(state): State<Arc<AppState>>,
) -> Json<GithubPrCurrentChecksResponse> {
    let ctx = match github_gh_context(&state).await {
        Ok(c) => c,
        Err(e) => {
            return Json(GithubPrCurrentChecksResponse {
                data: GithubPrCurrentChecksData::default(),
                error: Some(e),
            });
        }
    };
    let GithubGhContext {
        allowed_commands,
        max_output_len,
        work_dir,
    } = ctx;
    match tokio::task::spawn_blocking(move || {
        github_pr_current_checks(max_output_len, &allowed_commands, &work_dir)
    })
    .await
    {
        Ok(Ok(data)) => Json(GithubPrCurrentChecksResponse { data, error: None }),
        Ok(Err(e)) => Json(GithubPrCurrentChecksResponse {
            data: GithubPrCurrentChecksData::default(),
            error: Some(e),
        }),
        Err(e) => Json(GithubPrCurrentChecksResponse {
            data: GithubPrCurrentChecksData::default(),
            error: Some(format!("PR checks 查询失败：{e}")),
        }),
    }
}
