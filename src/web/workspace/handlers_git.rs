//! `GET /workspace/git`：只读 git 状态投影的 HTTP 薄壳。
//!
//! 解析、探测与状态映射都在 [`super::git_projection`]；这里只负责工作区根解析、
//! 缓存读写（TTL + 最小 single-flight）与响应组装。

use axum::Json;
use axum::extract::{Query, State};

use crate::cm_api_contract::error_codes;
use crate::cm_api_contract::workspace::{
    WorkspaceGitQuery, WorkspaceGitSource, WorkspaceGitState, WorkspaceGitStatusResponse,
};
use crate::web::app_state::{AppStateHttpCore, CachedWorkspaceGitStatus, WorkspaceGitCache};
use crate::web::workspace::git_projection::{
    self, CACHE_TTL, PROBE_TIMEOUT, ProbeOutcome, Projection, response_from_outcome,
};
use crate::web::workspace::handlers::effective_workspace_base_canonical;

/// 返回当前工作区的只读 git 状态投影。
///
/// 一律 200：可用性靠 `state` 表达（`ok` / `not_a_repo` / `git_unavailable` / `failed`）。
/// `?refresh=true` 只跳过缓存**读**，仍回写缓存 —— 因此该 GET 保持幂等。
pub async fn workspace_git_handler(
    State(http): State<AppStateHttpCore>,
    Query(query): Query<WorkspaceGitQuery>,
) -> Json<WorkspaceGitStatusResponse> {
    let raw_root = http.effective_workspace_path().await;
    if raw_root.trim().is_empty() {
        return Json(workspace_level_failure(
            error_codes::WORKSPACE_NOT_SET,
            "尚未设置工作区",
        ));
    }
    let root = match effective_workspace_base_canonical(&http).await {
        Ok(p) => p,
        Err(e) => {
            return Json(workspace_level_failure(
                error_codes::WORKSPACE_OUT_OF_ROOTS,
                &e.user_message(),
            ));
        }
    };
    let root_key = root.to_string_lossy().to_string();
    let cache = http.workspace_git_cache.clone();
    let force = query.refresh.unwrap_or(false);

    if !force
        && let Some(hit) = read_fresh(&cache, &root_key)
    {
        return Json(hit);
    }

    // 最小 single-flight：TTL 到期瞬间的并发请求不应各自 fork 两条 git 命令。
    // 本仓没有通用 single-flight 设施（`per_turn_flight` 是单回合状态镜像），锁 + 双重检查足够。
    let _probe_guard = cache.probe_lock.lock().await;
    if !force
        && let Some(hit) = read_fresh(&cache, &root_key)
    {
        return Json(hit);
    }

    let outcome = probe_with_timeout(root).await;
    match outcome {
        ProbeOutcome::Ok(Projection { .. })
        | ProbeOutcome::NotARepo
        | ProbeOutcome::GitUnavailable => {
            let response = response_from_outcome(
                outcome,
                git_projection::now_unix(),
                WorkspaceGitSource::Probe,
            );
            // 明确结论（含 not_a_repo / git_unavailable）写缓存，省掉 TTL 内的重复 fork。
            write_cache(&cache, &root_key, &response);
            Json(response)
        }
        ProbeOutcome::Failed { .. } => {
            // 重探失败时优先沿用旧值，避免一次超时把侧栏打成 failed。
            if let Some(stale) = read_stale(&cache, &root_key) {
                return Json(stale);
            }
            // 无旧值可兜底：不写缓存，避免把一次瞬时失败固化一整个 TTL。
            Json(response_from_outcome(
                outcome,
                git_projection::now_unix(),
                WorkspaceGitSource::Probe,
            ))
        }
    }
}

/// 工作区根层面的失败（未设置 / 越界），不经 probe。
fn workspace_level_failure(code: &str, message: &str) -> WorkspaceGitStatusResponse {
    WorkspaceGitStatusResponse {
        state: WorkspaceGitState::Failed,
        reason_code: Some(code.to_string()),
        message: Some(message.to_string()),
        head: None,
        upstream: None,
        ahead: None,
        behind: None,
        dirty: None,
        counts: None,
        computed_at_unix: git_projection::now_unix(),
        source: WorkspaceGitSource::Probe,
    }
}

/// 带墙钟上限的探测。
///
/// `spawn_blocking` 无法被取消，超时后那条线程仍会跑完再丢弃结果 —— 这是可接受的：
/// 单次探测上限 2s，且 single-flight 已把并发压成一次。
async fn probe_with_timeout(root: std::path::PathBuf) -> ProbeOutcome {
    let handle = tokio::task::spawn_blocking(move || git_projection::probe(&root));
    match tokio::time::timeout(PROBE_TIMEOUT, handle).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(join_error)) => ProbeOutcome::Failed {
            reason_code: error_codes::GIT_PROBE_FAILED,
            message: format!("探测任务失败：{join_error}"),
        },
        Err(_) => ProbeOutcome::Failed {
            reason_code: error_codes::GIT_PROBE_TIMEOUT,
            message: "git 探测超时".to_string(),
        },
    }
}

/// TTL 内的缓存命中（`source` 改写为 `cache`，`computed_at_unix` 保持探测时刻）。
fn read_fresh(cache: &WorkspaceGitCache, root_key: &str) -> Option<WorkspaceGitStatusResponse> {
    let guard = cache.entry.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.workspace_root != root_key || entry.computed_at.elapsed() >= CACHE_TTL {
        return None;
    }
    let mut response = entry.projection.clone();
    response.source = WorkspaceGitSource::Cache;
    Some(response)
}

/// 任意时长的旧值（供重探失败时兜底），`source` 改写为 `stale_cache`。
fn read_stale(cache: &WorkspaceGitCache, root_key: &str) -> Option<WorkspaceGitStatusResponse> {
    let guard = cache.entry.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.workspace_root != root_key {
        return None;
    }
    let mut response = entry.projection.clone();
    response.source = WorkspaceGitSource::StaleCache;
    Some(response)
}

/// 写入缓存。锁中毒时静默跳过：缓存坏掉不该让只读端点失败。
fn write_cache(cache: &WorkspaceGitCache, root_key: &str, response: &WorkspaceGitStatusResponse) {
    if let Ok(mut guard) = cache.entry.lock() {
        *guard = Some(CachedWorkspaceGitStatus {
            workspace_root: root_key.to_string(),
            computed_at: std::time::Instant::now(),
            projection: response.clone(),
        });
    }
}
